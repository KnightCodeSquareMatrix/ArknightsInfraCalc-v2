use std::collections::HashSet;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use serde::Serialize;

use crate::efficiency::Efficiency;
use crate::error::Result;
use crate::layout::LayoutContext;
use crate::pool::PowerPool;
use crate::power::{aggregate_power_efficiency, solve_power, PowerRoomInput};
use crate::skill_table::SkillTable;

/// 虚拟发电站折算为制造纸面效率的历史解释系数。
///
/// 当前发电站搜索排序 **不使用** 该系数，只按 `charge_speed_pct` 贪心。
/// 若后续需要让虚拟发电影响某个局部排序，应新增命名 policy。
pub const VIRTUAL_POWER_MANU_EQUIV: f64 = 30.0;

/// 发电站评分的完整分解，展示充能速度与虚拟发电各自的值。
///
/// 发电评分公式：
/// ```text
/// score = charge_speed_pct
/// ```
/// 虚拟发电站在制造 resolve 时通过 layout 无人机加速体现价值，不在此预支。
#[derive(Debug, Clone, Serialize, Default)]
pub struct PowerEfficiencyBreakdown {
    pub base_efficiency: Efficiency,
    pub skill_efficiency: Efficiency,
    pub ramp_efficiency: Efficiency,
    pub final_efficiency: Efficiency,
    /// 虚拟发电站数
    pub virtual_power_produced: f64,
    /// vpower × VIRTUAL_POWER_MANU_EQUIV
    pub virtual_power_equiv: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PowerSearchHit {
    pub name: String,
    pub final_efficiency: Efficiency,
    pub mood_drain_delta: f64,
    /// 晨曦等本班产出的虚拟发电站（写入 layout 前快照）。
    pub virtual_power_produced: f64,
    /// 评分明细分解
    pub breakdown: PowerEfficiencyBreakdown,
}

/// 单站贪心排序：纯充能速度 %。
///
/// 虚拟发电站的价值在制造站 resolve 时通过 layout 无人机加速自然体现，不在此预支。
fn power_efficiency_sort_key(hit: &PowerSearchHit) -> Efficiency {
    hit.final_efficiency
}

fn low_priority_power_name(name: &str) -> bool {
    matches!(
        name,
        "Castle-3" | "Friston-3" | "Lancet-2" | "PhonoR-0" | "THRM-EX" | "正义骑士号"
    )
}

fn power_hit_precedes(a: &PowerSearchHit, b: &PowerSearchHit) -> bool {
    match power_efficiency_sort_key(a).cmp(&power_efficiency_sort_key(b)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => low_priority_power_name(&a.name)
            .cmp(&low_priority_power_name(&b.name))
            .then_with(|| a.name.cmp(&b.name))
            .is_lt(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PowerStationAssignment {
    pub station_index: usize,
    pub hit: PowerSearchHit,
}

#[derive(Debug, Clone, Serialize)]
pub struct PowerSearchReport {
    pub assignments: Vec<PowerStationAssignment>,
    pub total_efficiency: Efficiency,
    pub evaluated: u64,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct PowerSearchOptions {
    pub station_count: u8,
    /// 调用方本轮实际要填充的房间，顺序与最终 assignment 一致。
    pub station_rooms: Vec<String>,
    pub mood: f64,
    pub shift_hours: f64,
    pub layout: LayoutContext,
}

impl Default for PowerSearchOptions {
    fn default() -> Self {
        Self {
            station_count: 3,
            station_rooms: Vec::new(),
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
    let mut perf = crate::profile::start_search_perf(crate::profile::SearchOperation::Power);
    let mut used = HashSet::new();
    let mut assignments = Vec::new();
    let mut evaluated = 0u64;
    perf.mark_live_started();

    let mut selected_layout = options.layout.clone();
    let rooms = options.station_rooms.clone();
    for station in 0..options.station_count as usize {
        selected_layout.candidate_room = rooms.get(station).cloned();
        let mut best: Option<PowerSearchHit> = None;
        for entry in &pool.entries {
            if used.contains(&entry.name) {
                continue;
            }
            let members = vec![(
                entry.name.clone(),
                entry.elite,
                entry.buff_ids.clone(),
                entry.tags.clone(),
            )];
            if !crate::layout::admission::candidate_admitted(
                &selected_layout,
                "power",
                members.clone(),
                table,
            ) {
                continue;
            }
            let shift_hours = crate::mood::room_work_window(
                &options.layout,
                crate::layout::FacilityKind::PowerPlant,
                None,
                &members,
                options.shift_hours,
            )?;
            let mut layout = options.layout.clone();
            layout.drone_cap = layout.drone_cap.max(135);
            let input = PowerRoomInput {
                operator: entry.to_power_operator(),
                mood: options.mood,
                shift_hours,
                layout,
            };
            let result = solve_power(&input, table)?;
            evaluated += 1;
            let hit = PowerSearchHit {
                name: entry.name.clone(),
                final_efficiency: result.final_efficiency,
                mood_drain_delta: result.mood_drain_delta,
                virtual_power_produced: result.virtual_power_produced,
                breakdown: PowerEfficiencyBreakdown {
                    base_efficiency: result.base_efficiency,
                    skill_efficiency: result.skill_efficiency,
                    ramp_efficiency: result.ramp_efficiency,
                    final_efficiency: result.final_efficiency,
                    virtual_power_produced: result.virtual_power_produced,
                    virtual_power_equiv: result.virtual_power_produced * VIRTUAL_POWER_MANU_EQUIV,
                },
            };
            if best.as_ref().is_none_or(|b| power_hit_precedes(&hit, b)) {
                best = Some(hit);
            }
        }
        let Some(hit) = best else { break };
        if selected_layout.auto_rotation {
            let entry = pool.entry(&hit.name).expect("selected power pool entry");
            let members = vec![(
                entry.name.clone(),
                entry.progress.elite,
                entry.buff_ids.clone(),
                entry.tags.clone(),
            )];
            if let Some(room) = rooms.get(station).and_then(|id| {
                selected_layout
                    .station_presence
                    .iter_mut()
                    .find(|room| room.room == *id)
            }) {
                room.members = members;
            } else {
                selected_layout
                    .station_presence
                    .push(crate::layout::admission::StationPresence {
                        room: format!("selected_power_{station}"),
                        kind: crate::layout::FacilityKind::PowerPlant,
                        level: 3,
                        facility: "power".into(),
                        members,
                        support_dependencies: Vec::new(),
                    });
            }
        }
        used.insert(hit.name.clone());
        assignments.push(PowerStationAssignment {
            station_index: station,
            hit,
        });
    }

    let report = PowerSearchReport {
        total_efficiency: aggregate_power_efficiency(
            assignments
                .iter()
                .map(|assignment| assignment.hit.final_efficiency),
        ),
        assignments,
        evaluated,
        elapsed: start.elapsed(),
    };
    perf.finish(
        0,
        evaluated,
        evaluated,
        report.assignments.len() as u64,
        false,
        crate::profile::SearchPhaseDurations {
            evaluate: report.elapsed,
            ..Default::default()
        },
    );
    Ok(report)
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
                final_efficiency: result.final_efficiency,
                mood_drain_delta: result.mood_drain_delta,
                virtual_power_produced: result.virtual_power_produced,
                breakdown: PowerEfficiencyBreakdown {
                    base_efficiency: result.base_efficiency,
                    skill_efficiency: result.skill_efficiency,
                    ramp_efficiency: result.ramp_efficiency,
                    final_efficiency: result.final_efficiency,
                    virtual_power_produced: result.virtual_power_produced,
                    virtual_power_equiv: result.virtual_power_produced * VIRTUAL_POWER_MANU_EQUIV,
                },
            })
        })
        .collect();

    hits.sort_by(|a, b| {
        power_efficiency_sort_key(b)
            .cmp(&power_efficiency_sort_key(a))
            .then_with(|| low_priority_power_name(&a.name).cmp(&low_priority_power_name(&b.name)))
            .then_with(|| a.name.cmp(&b.name))
    });
    hits.truncate(top_k);
    Ok(hits)
}
