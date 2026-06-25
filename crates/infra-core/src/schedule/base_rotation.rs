use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{
    assign_shift, pinned_assignment, resolve_base, rotating_workers, AssignBaseOptions,
    AssignShiftMode, BaseAssignment, BaseBlueprint,
};
use crate::manufacture::input::ManuRoomInput;
use crate::manufacture::solve_manufacture;
use crate::operbox::OperBox;
use crate::power::{solve_power, PowerRoomInput};
use crate::skill_table::SkillTable;
use crate::trade::input::TradeRoomInput;
use crate::trade::solve_trade_with_shift;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BaseShiftRole {
    Peak,
    Recovery,
}

/// 单房间评分快照（用于 CLI 逐房展示）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct RoomScoreLine {
    pub room_id: String,
    /// 仅贸易站填写：effective_eff_multiplier（产出倍率）
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub trade_score: f64,
    /// 仅贸易站填写：纸面效率%（order_eff_total，含人头+技能+全局）
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub trade_pct: f64,
    /// 仅贸易站填写：技能效率%（order_eff_skill，不含人头/全局）
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub trade_skill_pct: f64,
    /// 仅制造站填写：社区展示口径，即技能生产力 `prod_skill`（不含人头/全局注入）。
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub manu_score: f64,
    /// 仅发电站填写：charge_speed_pct
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub power_score: f64,
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ShiftScores {
    pub trade_score: f64,
    pub manu_prod_sum: f64,
    /// 发电站充能速度 % 合计（按 `shift_hours` 评估，含空构爬升）。
    pub power_charge_sum: f64,
    /// 各房间评分明细（按 SHIFT_STATION_ORDER 顺序）。
    pub room_lines: Vec<RoomScoreLine>,
}

impl ShiftScores {
    /// 贸易分按时长折算（不与制造/发电混合量纲）。
    pub fn weighted_trade(&self, shift_hours: f64) -> f64 {
        self.trade_score * (shift_hours / 24.0)
    }
    /// 制造产量按时长折算。
    pub fn weighted_manu(&self, shift_hours: f64) -> f64 {
        self.manu_prod_sum * (shift_hours / 24.0)
    }
    /// 发电充能% 按时长折算。
    pub fn weighted_power(&self, shift_hours: f64) -> f64 {
        self.power_charge_sum * (shift_hours / 24.0)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BaseShiftPlan {
    pub index: usize,
    pub role: BaseShiftRole,
    pub assignment: BaseAssignment,
    pub scores: ShiftScores,
    pub rotating_workers: Vec<String>,
    /// `Some(0)` when this shift reuses shift 1 (A-B-A).
    pub reused_from_shift: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaseRotationReport {
    pub shifts: Vec<BaseShiftPlan>,
    pub elapsed: Duration,
}

fn workers_sorted(set: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<_> = set.iter().cloned().collect();
    v.sort();
    v
}

fn assert_disjoint(a: &HashSet<String>, b: &HashSet<String>, label: &str) -> Result<()> {
    let overlap: Vec<String> = a.intersection(b).cloned().collect();
    if overlap.is_empty() {
        Ok(())
    } else {
        Err(Error::msg(format!("{label} 轮换岗干员重合: {overlap:?}")))
    }
}

/// 对编制逐房求贸易/制造/发电纸面分（满心情）；`shift_hours` 影响发电爬升与产出折算。
pub fn score_base_assignment(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    shift_hours: f64,
    durin_plan: Option<u8>,
) -> Result<ShiftScores> {
    let resolved = resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        24.0,
        durin_plan,
    )?;

    let mut trade_score = 0.0;
    let mut manu_prod_sum = 0.0;
    let mut power_charge_sum = 0.0;
    let mut room_lines: Vec<RoomScoreLine> = Vec::new();

    for room in &resolved.trade_rooms {
        let mut line = RoomScoreLine {
            room_id: room.id.0.clone(),
            ..RoomScoreLine::default()
        };
        if let Some(snapshot) = assignment.efficiency_in(&room.id).filter(|s| s.is_trade()) {
            trade_score += snapshot.trade_score;
            line.trade_score = snapshot.trade_score;
            line.trade_pct = snapshot.trade_pct;
            line.trade_skill_pct = snapshot.trade_skill_pct;
        } else if !room.operators.is_empty() {
            let input = TradeRoomInput {
                level: room.level,
                operators: room.operators.clone(),
                order_count: None,
                mood: 24.0,
                gold_production_lines: Some(resolved.gold_manu_line_count()),
                durin_virtual_lines: None,
                human_fireworks: None,
                layout: Arc::new(room.layout.clone()),
                active_order_kind: room.order,
            };
            let result = solve_trade_with_shift(&input, table, shift_hours)?;
            trade_score += result.effective_eff_multiplier;
            line.trade_score = result.effective_eff_multiplier;
            line.trade_pct = result.order_eff_total;
            // 社区惯例：逐房展示只看技能面，不含人头与中枢全局注入。
            line.trade_skill_pct = result.order_eff_skill;
        }
        room_lines.push(line);
    }

    for room in &resolved.manu_rooms {
        let mut line = RoomScoreLine {
            room_id: room.id.0.clone(),
            ..RoomScoreLine::default()
        };
        if let Some(snapshot) = assignment
            .efficiency_in(&room.id)
            .filter(|s| s.is_manufacture())
        {
            manu_prod_sum += snapshot.manu_prod_total;
            line.manu_score = snapshot.manu_prod_skill;
        } else if !room.operators.is_empty() {
            let input = ManuRoomInput {
                level: room.level,
                operators: room.operators.clone(),
                active_recipe: room.recipe,
                mood: 24.0,
                layout: Arc::new(room.layout.clone()),
            };
            let result = solve_manufacture(&input, table)?;
            let prod = result.prod_total;
            manu_prod_sum += prod;
            // 社区惯例：逐房展示只看技能面，不含人头与中枢全局注入。
            line.manu_score = result.prod_skill;
        }
        room_lines.push(line);
    }

    for room in &resolved.power_rooms {
        let mut line = RoomScoreLine {
            room_id: room.id.0.clone(),
            ..RoomScoreLine::default()
        };
        let score =
            if let Some(snapshot) = assignment.efficiency_in(&room.id).filter(|s| s.is_power()) {
                snapshot.power_charge_speed_pct
            } else {
                let input = PowerRoomInput {
                    operator: room.operator.clone(),
                    mood: 24.0,
                    shift_hours,
                    layout: room.layout.clone(),
                };
                solve_power(&input, table)?.charge_speed_pct
            };
        power_charge_sum += score;
        line.power_score = score;
        room_lines.push(line);
    }

    Ok(ShiftScores {
        trade_score,
        manu_prod_sum,
        power_charge_sum,
        room_lines,
    })
}

/// 全基建三班 A-B-A：高峰班 → 恢复班（池修剪）→ 复用高峰班；中枢/宿舍三班钉死。
pub fn schedule_base_rotation_a_b_a(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<BaseRotationReport> {
    let start = Instant::now();
    blueprint.validate()?;

    let durin_plan = operbox.durin_dorm_planning_count(instances);

    let peak_assignment = assign_shift(
        blueprint,
        operbox,
        instances,
        table,
        options,
        AssignShiftMode::Peak,
        &BaseAssignment::default(),
    )?;
    let peak_rotating = rotating_workers(&peak_assignment, blueprint);
    if peak_rotating.is_empty() {
        return Err(Error::msg("高峰班无贸易/制造/发电岗位"));
    }

    let pinned = pinned_assignment(&peak_assignment, blueprint);
    let recovery_operbox = operbox.excluding(&peak_rotating);

    let recovery_assignment = assign_shift(
        blueprint,
        &recovery_operbox,
        instances,
        table,
        options,
        AssignShiftMode::Recovery,
        &pinned,
    )?;
    let recovery_rotating = rotating_workers(&recovery_assignment, blueprint);
    assert_disjoint(&peak_rotating, &recovery_rotating, "高峰班与恢复班")?;

    let peak_scores = score_base_assignment(
        blueprint,
        &peak_assignment,
        instances,
        table,
        24.0,
        Some(durin_plan),
    )?;
    let recovery_scores = score_base_assignment(
        blueprint,
        &recovery_assignment,
        instances,
        table,
        24.0,
        Some(durin_plan),
    )?;

    let shift1 = BaseShiftPlan {
        index: 0,
        role: BaseShiftRole::Peak,
        assignment: peak_assignment.clone(),
        scores: peak_scores.clone(),
        rotating_workers: workers_sorted(&peak_rotating),
        reused_from_shift: None,
    };

    let shift2 = BaseShiftPlan {
        index: 1,
        role: BaseShiftRole::Recovery,
        assignment: recovery_assignment,
        scores: recovery_scores,
        rotating_workers: workers_sorted(&recovery_rotating),
        reused_from_shift: None,
    };

    // shift3 复用 shift1 的 peak_assignment 评分，避免重复求解。
    let shift3_scores = peak_scores;
    let shift3 = BaseShiftPlan {
        index: 2,
        role: BaseShiftRole::Peak,
        assignment: peak_assignment,
        scores: shift3_scores,
        rotating_workers: workers_sorted(&peak_rotating),
        reused_from_shift: Some(0),
    };

    Ok(BaseRotationReport {
        shifts: vec![shift1, shift2, shift3],
        elapsed: start.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::assignment_operator_names;
    use crate::layout::RoomEfficiencySnapshot;
    use crate::operbox::{default_operbox_gongsun_path, OperBox};
    use crate::skill_table::{data_path, default_skill_table_path, SkillTable};

    fn fixtures_243_2gold() -> (BaseBlueprint, OperBox, OperatorInstances, SkillTable) {
        let blueprint =
            BaseBlueprint::load(&data_path("layout/243_use_this_.json").unwrap()).unwrap();
        let operbox = OperBox::load(&data_path("schedule_243/operbox_ideal_e2.json").unwrap())
            .or_else(|_| OperBox::load(&default_operbox_gongsun_path().unwrap()))
            .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        (blueprint, operbox, instances, table)
    }

    use crate::instances::default_instances_path;

    #[test]
    fn base_rotation_aba_disjoint_and_reuse() {
        let (blueprint, operbox, instances, table) = fixtures_243_2gold();
        let report = schedule_base_rotation_a_b_a(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.shifts.len(), 3);
        assert_eq!(report.shifts[2].reused_from_shift, Some(0));

        let w1: HashSet<_> = report.shifts[0].rotating_workers.iter().cloned().collect();
        let w2: HashSet<_> = report.shifts[1].rotating_workers.iter().cloned().collect();
        assert!(w1.is_disjoint(&w2));

        for shift in &report.shifts {
            let names = assignment_operator_names(&shift.assignment);
            assert_eq!(
                names.len(),
                shift
                    .assignment
                    .rooms
                    .iter()
                    .map(|r| r.operators.len())
                    .sum::<usize>(),
                "shift {} has duplicate operators",
                shift.index + 1
            );
        }
    }

    // 迷迭香+黑键「上 2 休 1」已迁至 αβγ：`team_rotation::team_rotation_rosemary_blackkey_shift_bind`。
    // A-B-A 已废弃，不再维护该约束。
    #[test]
    fn base_rotation_peak_beats_recovery_on_trade_paper() {
        let (blueprint, operbox, instances, table) = fixtures_243_2gold();
        let report = schedule_base_rotation_a_b_a(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions::default(),
        )
        .unwrap();
        assert!(
            report.shifts[0].scores.trade_score >= report.shifts[1].scores.trade_score,
            "peak trade {:.3} should be >= recovery {:.3}",
            report.shifts[0].scores.trade_score,
            report.shifts[1].scores.trade_score
        );
    }

    #[test]
    fn score_base_assignment_uses_room_efficiency_snapshot() {
        let (blueprint, _operbox, instances, table) = fixtures_243_2gold();
        let mut assignment = BaseAssignment::default();
        assignment.set_room_with_efficiency(
            "manu_1",
            vec![
                crate::layout::AssignedOperator::new("芬", 1),
                crate::layout::AssignedOperator::new("克洛丝", 1),
                crate::layout::AssignedOperator::new("泡普卡", 1),
            ],
            Some(RoomEfficiencySnapshot {
                manu_prod_total: 333.0,
                manu_prod_skill: 30.0,
                ..RoomEfficiencySnapshot::default()
            }),
        );

        let scores =
            score_base_assignment(&blueprint, &assignment, &instances, &table, 24.0, None).unwrap();
        let line = scores
            .room_lines
            .iter()
            .find(|line| line.room_id == "manu_1")
            .expect("manu_1 line");
        assert_eq!(scores.manu_prod_sum, 333.0);
        assert_eq!(line.manu_score, 30.0);
    }
}
