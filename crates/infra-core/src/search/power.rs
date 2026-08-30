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
    let mut perf = crate::profile::start_search_perf(crate::profile::SearchOperation::Power);
    let mut used = HashSet::new();
    let mut assignments = Vec::new();
    let mut evaluated = 0u64;
    perf.mark_live_started();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_search_score_is_charge_speed_sort_key() {
        let hit = PowerSearchHit {
            name: "a".into(),
            final_efficiency: Efficiency::from_decimal(1.200),
            mood_drain_delta: 0.0,
            virtual_power_produced: 1.0,
            breakdown: PowerEfficiencyBreakdown {
                base_efficiency: Efficiency::ONE,
                skill_efficiency: Efficiency::from_decimal(0.200),
                final_efficiency: Efficiency::from_decimal(1.200),
                virtual_power_produced: 1.0,
                virtual_power_equiv: VIRTUAL_POWER_MANU_EQUIV,
                ..Default::default()
            },
        };
        assert_eq!(power_efficiency_sort_key(&hit), hit.final_efficiency);
        assert_eq!(hit.final_efficiency, hit.breakdown.final_efficiency);
    }
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::layout::resolve_search_baseline_layout;
    use crate::operbox::default_operbox_full_e2_path;
    use crate::operbox::OperBox;
    use crate::pool::{build_power_pool, PowerPool, PowerPoolEntry};
    use crate::roster::OperatorProgress;
    use crate::skill_table::default_skill_table_path;
    use crate::skill_table::SkillTable;

    #[test]
    fn greyy2_uses_prts_full_drone_cap_and_virtual_power() {
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
        let greyy2 = hits
            .iter()
            .find(|h| h.name == "承曦格雷伊")
            .expect("承曦格雷伊");
        let greyy = hits.iter().find(|h| h.name == "格雷伊").expect("格雷伊");
        assert!(greyy2.virtual_power_produced > 0.0, "E2 晨曦应产出虚拟发电");
        assert!(
            greyy2.final_efficiency > greyy.final_efficiency,
            "满清理 235 无人机上限: 承曦格雷伊=23.5% > 格雷伊=20%"
        );
    }

    #[test]
    fn power_assignment_fills_all_stations_and_greyy2_is_included() {
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
        let names: Vec<_> = report
            .assignments
            .iter()
            .map(|a| a.hit.name.as_str())
            .collect();
        assert!(
            names.iter().any(|n| *n == "承曦格雷伊"),
            "满清理 235 无人机上限时，承曦格雷伊应凭 23.5% 充能进入前 3 站: {names:?}"
        );
    }

    fn power_entry(name: &str, buff_ids: &[&str]) -> PowerPoolEntry {
        PowerPoolEntry {
            name: name.to_string(),
            elite: 2,
            progress: OperatorProgress::elite_only(2),
            buff_ids: buff_ids.iter().map(|id| id.to_string()).collect(),
            tags: vec![],
            flat_charge_hint: 0.0,
            has_l2_delegate: false,
            tier: crate::layout::tier::OperatorTier::Standalone,
        }
    }

    #[test]
    fn power_assignment_tie_prefers_plain_charger_over_friston_combo() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let pool = PowerPool {
            entries: vec![
                power_entry("Friston-3", &["power_rec_spd[000]", "power_rec_spd_P[000]"]),
                power_entry("协律", &["power_rec_spd[009]"]),
            ],
            skipped: vec![],
        };
        let mut layout = LayoutContext::search_baseline();
        layout.base_workforce = vec!["凯尔希".to_string()];
        let opts = PowerSearchOptions {
            station_count: 1,
            layout,
            ..Default::default()
        };

        let report = search_power_assignment(&pool, &table, &opts).unwrap();

        assert_eq!(report.assignments[0].hit.name, "协律");
        assert_eq!(
            report.assignments[0].hit.final_efficiency,
            Efficiency::from_decimal(1.150)
        );
    }
}
