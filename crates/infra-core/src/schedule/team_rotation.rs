use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{
    assign_shift_with_plan, assign_team_gamma_half, blackkey_witch_same_trade_room,
    pinned_assignment, resolve_base, AssignBaseOptions, AssignShiftMode, AssignmentPlan,
    BaseAssignment, BaseBlueprint, FacilityKind, RoomId,
};
use crate::operbox::OperBox;
use crate::pool::{build_manufacture_pool, build_power_pool, build_trade_pool};
use crate::skill_table::SkillTable;

use super::base_rotation::{score_base_assignment, ShiftScores};
use super::shift_bind::align_shift_binds_in_halves;

/// αβγ 三队标签。每班两队上岗、一队休息；设施每班全部满编（不空转）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamLabel {
    Alpha,
    Beta,
    Gamma,
}

impl TeamLabel {
    pub const ALL: [TeamLabel; 3] = [TeamLabel::Alpha, TeamLabel::Beta, TeamLabel::Gamma];
}

/// 一支队伍（轮休 cohort）：休息一个班次的一批干员。
#[derive(Debug, Clone, Serialize)]
pub struct TeamAssignment {
    pub label: TeamLabel,
    pub operators: Vec<String>,
}

/// 单个班次结果：当班两队合起来铺满全部设施。
#[derive(Debug, Clone, Serialize)]
pub struct TeamShiftResult {
    pub index: usize,
    pub duration_hours: f64,
    pub active_teams: Vec<TeamLabel>,
    pub resting_team: TeamLabel,
    pub assignment: BaseAssignment,
    pub scores: ShiftScores,
    /// 贸易分按时长折算（三类各自独立，不混合量纲）。
    pub weighted_trade: f64,
    /// 制造产量按时长折算。
    pub weighted_manu: f64,
    /// 发电充能% 按时长折算。
    pub weighted_power: f64,
}

/// 三类各自的每日加权产出（贸易/制造/发电分开，不相加）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct DailyTotals {
    pub trade: f64,
    pub manu: f64,
    pub power: f64,
}

/// αβγ 三队轮换报告。
#[derive(Debug, Clone, Serialize)]
pub struct TeamRotationReport {
    /// peak 班编排计划（只读；α/β 切半与 γ plain 贸易均据此对齐）。
    pub peak_plan: AssignmentPlan,
    pub teams: Vec<TeamAssignment>,
    pub shifts: Vec<TeamShiftResult>,
    /// 三类各自的每日加权产出（12h×αβ + 6h×βγ + 6h×γα，分别汇总）。
    pub daily: DailyTotals,
    pub elapsed: Duration,
}

/// 生产设施一个半区（trade/manu/power 各一组完整房间）。
#[derive(Debug, Clone, Default)]
pub struct FacilityHalf {
    pub trade: Vec<RoomId>,
    pub manu: Vec<RoomId>,
    pub power: Vec<RoomId>,
}

/// 把全部生产设施（贸易/制造/发电）按同类房间交替切成两半，尽量均衡负载。
fn split_production_facilities(blueprint: &BaseBlueprint) -> [FacilityHalf; 2] {
    let mut halves: [FacilityHalf; 2] = Default::default();
    for (i, room) in blueprint.rooms_of(FacilityKind::TradePost).iter().enumerate() {
        halves[i % 2].trade.push(room.id.clone());
    }
    for (i, room) in blueprint.rooms_of(FacilityKind::Factory).iter().enumerate() {
        halves[i % 2].manu.push(room.id.clone());
    }
    for (i, room) in blueprint.rooms_of(FacilityKind::PowerPlant).iter().enumerate() {
        halves[i % 2].power.push(room.id.clone());
    }
    halves
}

/// γ 替补半区：贸易 plain 贪心（不重搜 meta），制造/发电站绑定搜索。
#[allow(clippy::too_many_arguments)]
fn assign_gamma_half(
    blueprint: &BaseBlueprint,
    pools: &ProductionPools,
    table: &SkillTable,
    layout: &crate::layout::LayoutContext,
    options: &AssignBaseOptions,
    half: &FacilityHalf,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    assign_team_gamma_half(
        blueprint,
        &pools.trade,
        &pools.manu,
        &pools.power,
        table,
        layout,
        options,
        &half.trade,
        &half.manu,
        &half.power,
        assignment,
        used,
    )
}

fn production_half_from_peak(peak: &BaseAssignment, half: &FacilityHalf) -> BaseAssignment {
    let mut half_assignment = BaseAssignment::default();
    for room_id in half
        .trade
        .iter()
        .chain(half.manu.iter())
        .chain(half.power.iter())
    {
        let ops = peak.operators_in(room_id);
        if !ops.is_empty() {
            half_assignment.set_room(room_id.clone(), ops.to_vec());
        }
    }
    half_assignment
}

struct ProductionPools {
    trade: crate::pool::TradePool,
    manu: crate::pool::ManuPool,
    power: crate::pool::PowerPool,
}

fn operators_of(assignment: &BaseAssignment) -> Vec<String> {
    let mut names: Vec<String> = assignment
        .rooms
        .iter()
        .flat_map(|r| r.operators.iter().map(|o| o.name.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn merge_rooms(target: &mut BaseAssignment, source: &BaseAssignment) {
    for room in &source.rooms {
        target.set_room(room.room_id.clone(), room.operators.clone());
    }
}

/// 全基建 αβγ 三队均衡轮休排班（公孙长乐替补池模型）。
///
/// - **设施每班全部满编，绝不空转**：每班由当班两队合力铺满所有贸易/制造/发电站。
/// - 生产设施切成两半 H1/H2：α 跑 H1、β 跑 H2；γ 作为轮换替补，第 2 班接 H1、第 3 班接 H2。
/// - 班次结构 12h + 6h + 6h；每队休息一个班次（α 休 S2、β 休 S3、γ 休 S1）。
/// - 中枢 / 宿舍为共享脚手架，三班钉死（体系绑定干员随脚手架；轮休细化为后续）。
pub fn schedule_team_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport> {
    let start = Instant::now();
    blueprint.validate()?;

    let durin_plan = operbox.durin_dorm_planning_count(instances);

    // 1) 参考高峰班 + 编排计划 → 取中枢/宿舍作为三班共享脚手架。
    let peak_result = assign_shift_with_plan(
        blueprint,
        operbox,
        instances,
        table,
        options,
        AssignShiftMode::Peak,
        &BaseAssignment::default(),
    )?;
    let peak = peak_result.assignment;
    let peak_plan = peak_result.plan;
    let shared = pinned_assignment(&peak, blueprint);
    let scaffold_used: HashSet<String> = operators_of(&shared).into_iter().collect();

    // 2) 以脚手架解算 layout（中枢 buff 生效），供生产搜索。
    let layout = resolve_base(
        blueprint,
        &shared,
        Some(instances),
        Some(table),
        options.mood,
        Some(durin_plan),
    )?
    .layout_snapshot();

    let pools = ProductionPools {
        trade: build_trade_pool(&operbox.trade_roster(instances), instances, table)?,
        manu: build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?,
        power: build_power_pool(&operbox.power_roster(instances), instances, table)?,
    };

    let [mut h1, mut h2] = split_production_facilities(blueprint);
    align_shift_binds_in_halves(&peak, operbox, &mut h1, &mut h2);

    // 3) α/β 从 peak 编制按 H1/H2 切半（保留编排已认领的 meta 锚点）；γ plain 贸易替补。
    let alpha = production_half_from_peak(&peak, &h1);
    let beta = production_half_from_peak(&peak, &h2);
    peak_plan
        .verify_registry_trade_in_alpha_beta(&alpha, &beta)
        .map_err(Error::msg)?;
    let mut used = scaffold_used.clone();
    for name in operators_of(&alpha).into_iter().chain(operators_of(&beta)) {
        used.insert(name);
    }

    // 4) γ 为替补：S2 接 H1、S3 接 H2，干员与 α/β 互斥（两次各自从剩余池取，可复用同人）。
    let used_ab = used.clone();

    let mut gamma_h1 = BaseAssignment::default();
    let mut used_g1 = used_ab.clone();
    assign_gamma_half(blueprint, &pools, table, &layout, options, &h1, &mut gamma_h1, &mut used_g1)?;

    let mut gamma_h2 = BaseAssignment::default();
    let mut used_g2 = used_ab.clone();
    assign_gamma_half(blueprint, &pools, table, &layout, options, &h2, &mut gamma_h2, &mut used_g2)?;

    // 队伍花名册（cohort）。
    let mut gamma_ops: Vec<String> = operators_of(&gamma_h1);
    gamma_ops.extend(operators_of(&gamma_h2));
    gamma_ops.sort();
    gamma_ops.dedup();
    let teams = vec![
        TeamAssignment { label: TeamLabel::Alpha, operators: operators_of(&alpha) },
        TeamAssignment { label: TeamLabel::Beta, operators: operators_of(&beta) },
        TeamAssignment { label: TeamLabel::Gamma, operators: gamma_ops },
    ];

    // 5) 组装三班（每班满编）并评分：
    //    S1(12h)=脚手架+α(H1)+β(H2)；S2(6h)=脚手架+β(H2)+γ(H1)；S3(6h)=脚手架+α(H1)+γ(H2)。
    let shift_specs: [(f64, [TeamLabel; 2], TeamLabel, [&BaseAssignment; 2]); 3] = [
        (12.0, [TeamLabel::Alpha, TeamLabel::Beta], TeamLabel::Gamma, [&alpha, &beta]),
        (6.0, [TeamLabel::Beta, TeamLabel::Gamma], TeamLabel::Alpha, [&beta, &gamma_h1]),
        (6.0, [TeamLabel::Gamma, TeamLabel::Alpha], TeamLabel::Beta, [&gamma_h2, &alpha]),
    ];

    let mut shifts = Vec::with_capacity(3);
    let mut daily = DailyTotals::default();
    for (index, (hours, active, resting, parts)) in shift_specs.into_iter().enumerate() {
        let mut assignment = shared.clone();
        for part in parts {
            merge_rooms(&mut assignment, part);
        }
        let scores =
            score_base_assignment(blueprint, &assignment, instances, table, hours, Some(durin_plan))?;
        let weighted_trade = scores.weighted_trade(hours);
        let weighted_manu = scores.weighted_manu(hours);
        let weighted_power = scores.weighted_power(hours);
        daily.trade += weighted_trade;
        daily.manu += weighted_manu;
        daily.power += weighted_power;
        shifts.push(TeamShiftResult {
            index,
            duration_hours: hours,
            active_teams: active.to_vec(),
            resting_team: resting,
            assignment,
            scores,
            weighted_trade,
            weighted_manu,
            weighted_power,
        });
    }

    Ok(TeamRotationReport {
        peak_plan,
        teams,
        shifts,
        daily,
        elapsed: start.elapsed(),
    })
}

/// 干员 → 所属队伍 的查表（输出层给每个设施打队伍标签用）。
pub fn operator_team_map(report: &TeamRotationReport) -> HashMap<String, TeamLabel> {
    let mut map = HashMap::new();
    for team in &report.teams {
        for op in &team.operators {
            map.entry(op.clone()).or_insert(team.label);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;
    use crate::layout::assign_shift;
    use crate::operbox::{default_operbox_full_e2_path, default_operbox_gongsun_path};
    use crate::skill_table::default_skill_table_path;

    fn fixtures() -> (BaseBlueprint, OperBox, OperatorInstances, SkillTable) {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap())
            .or_else(|_| OperBox::load(&default_operbox_gongsun_path().unwrap()))
            .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        (blueprint, operbox, instances, table)
    }

    #[test]
    fn team_rotation_fills_every_facility_each_shift() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(report.shifts.len(), 3);
        assert_eq!(report.teams.len(), 3);

        let production_rooms: Vec<&RoomId> = blueprint
            .rooms
            .iter()
            .filter(|r| {
                matches!(
                    r.kind,
                    FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
                )
            })
            .map(|r| &r.id)
            .collect();

        // 关键：每班每个生产设施都满编，绝不空转。
        for shift in &report.shifts {
            for room_id in &production_rooms {
                let ops = shift.assignment.operators_in(room_id);
                assert!(
                    !ops.is_empty(),
                    "shift {} 设施 {} 空转",
                    shift.index + 1,
                    room_id.0
                );
            }
            // 每班内部无重复干员。
            let mut seen = HashSet::new();
            for room in &shift.assignment.rooms {
                for op in &room.operators {
                    assert!(seen.insert(op.name.clone()), "shift {} dup {}", shift.index, op.name);
                }
            }
        }

        // 三队两两互斥。
        for i in 0..report.teams.len() {
            for j in (i + 1)..report.teams.len() {
                let a: HashSet<_> = report.teams[i].operators.iter().collect();
                let b: HashSet<_> = report.teams[j].operators.iter().collect();
                assert!(a.is_disjoint(&b), "teams {i} & {j} overlap");
            }
        }

        assert!((report.shifts[0].duration_hours - 12.0).abs() < f64::EPSILON);
        assert!(report.daily.trade > 0.0);
        assert!(report.daily.manu > 0.0);
    }

    #[test]
    fn team_rotation_carries_peak_plan() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.peak_plan.mode, AssignShiftMode::Peak);
        if operbox.owns("但书") {
            assert!(
                report.peak_plan.registry_system_ids().contains(&"docus_syracusa"),
                "peak_plan 应含但书链: {:?}",
                report.peak_plan.registry_system_ids()
            );
        }
    }

    #[test]
    fn team_rotation_carries_peak_blackkey_trade_station() {
        let (blueprint, operbox, instances, table) = fixtures();
        if !operbox.owns("黑键") {
            return;
        }
        let peak = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();
        let peak_has_blackkey = peak.rooms.iter().any(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
                && r.operators.iter().any(|o| o.name == "黑键")
        });
        if !peak_has_blackkey {
            return;
        }

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        let blackkey_in_rotation = report.shifts.iter().any(|shift| {
            shift.assignment.rooms.iter().any(|room| {
                blueprint
                    .rooms
                    .iter()
                    .any(|b| b.id == room.room_id && b.kind == FacilityKind::TradePost)
                    && room.operators.iter().any(|o| o.name == "黑键")
            })
        });
        assert!(
            blackkey_in_rotation,
            "peak 已认领黑键贸站时 team-rotation 应保留"
        );
        assert!(
            report.teams[0]
                .operators
                .iter()
                .chain(report.teams[1].operators.iter())
                .any(|n| n == "黑键"),
            "黑键应在 α 或 β 队: alpha={:?} beta={:?}",
            report.teams[0].operators,
            report.teams[1].operators
        );
        for shift in &report.shifts {
            assert!(
                !blackkey_witch_same_trade_room(&shift.assignment, &blueprint),
                "shift {} 黑键与巫恋不得同房",
                shift.index + 1
            );
        }
    }

    #[test]
    fn team_rotation_rosemary_blackkey_shift_bind() {
        use crate::schedule::shift_bind::{team_of_operator, verify_shift_binds};

        let (blueprint, operbox, instances, table) = fixtures();
        if !operbox.owns("迷迭香") || !operbox.owns("黑键") {
            return;
        }
        let peak = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();
        if !peak.rooms.iter().any(|r| r.operators.iter().any(|o| o.name == "迷迭香"))
            || !peak.rooms.iter().any(|r| r.operators.iter().any(|o| o.name == "黑键"))
        {
            return;
        }

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        verify_shift_binds(&report, &operbox, &peak).expect("迷迭香+黑键 应同上同下、上2休1");
        let team = team_of_operator(&report, "迷迭香").unwrap();
        assert_eq!(
            team_of_operator(&report, "黑键"),
            Some(team),
            "迷迭香与黑键应同队"
        );
    }

    /// γ 队贸易站不得抢占 peak α/β 已认领的 meta 干员（如但书、巫恋）。
    #[test]
    fn team_rotation_gamma_trade_disjoint_from_peak_meta() {
        const META_TRADE_OPS: &[&str] = &["但书", "巫恋", "龙舌兰", "可露希尔"];

        let (blueprint, operbox, instances, table) = fixtures();
        let peak = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();

        let peak_meta: HashSet<String> = peak
            .rooms
            .iter()
            .filter(|r| {
                blueprint
                    .rooms
                    .iter()
                    .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
            })
            .flat_map(|r| r.operators.iter().map(|o| o.name.clone()))
            .filter(|n| META_TRADE_OPS.contains(&n.as_str()))
            .collect();
        if peak_meta.is_empty() {
            return;
        }

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        let gamma_ops: HashSet<_> = report.teams[2].operators.iter().cloned().collect();
        for name in peak_meta {
            assert!(
                !gamma_ops.contains(&name),
                "γ 队不应含 peak meta 干员 {name}"
            );
        }
    }
}
