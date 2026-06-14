use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{
    assign_shift, pinned_assignment, rotating_workers, AssignBaseOptions, AssignShiftMode,
    BaseAssignment, BaseBlueprint, resolve_base,
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

#[derive(Debug, Clone, Default, Serialize)]
pub struct ShiftScores {
    pub trade_score: f64,
    pub manu_prod_sum: f64,
    /// 发电站充能速度 % 合计（按 `shift_hours` 评估，含空构爬升）。
    pub power_charge_sum: f64,
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
    for room in &resolved.trade_rooms {
        if room.operators.is_empty() {
            continue;
        }
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
        trade_score += solve_trade_with_shift(&input, table, shift_hours)?.effective_eff_multiplier;
    }

    let mut manu_prod_sum = 0.0;
    for room in &resolved.manu_rooms {
        if room.operators.is_empty() {
            continue;
        }
        let input = ManuRoomInput {
            level: room.level,
            operators: room.operators.clone(),
            active_recipe: room.recipe,
            mood: 24.0,
            layout: Arc::new(room.layout.clone()),
        };
        manu_prod_sum += solve_manufacture(&input, table)?.prod_total;
    }

    let mut power_charge_sum = 0.0;
    for room in &resolved.power_rooms {
        let input = PowerRoomInput {
            operator: room.operator.clone(),
            mood: 24.0,
            shift_hours,
            layout: room.layout.clone(),
        };
        power_charge_sum += solve_power(&input, table)?.charge_speed_pct;
    }

    Ok(ShiftScores {
        trade_score,
        manu_prod_sum,
        power_charge_sum,
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

    let peak_scores = score_base_assignment(blueprint, &peak_assignment, instances, table, 24.0, Some(durin_plan))?;
    let recovery_scores =
        score_base_assignment(blueprint, &recovery_assignment, instances, table, 24.0, Some(durin_plan))?;

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
                shift.assignment.rooms.iter().map(|r| r.operators.len()).sum::<usize>(),
                "shift {} has duplicate operators",
                shift.index + 1
            );
        }
    }

    #[test]
    fn rosemary_blackkey_bound_to_peak_shifts_rest_one() {
        // 文档 §6.1：迷迭香+黑键「同上同下，上 2 休 1」。
        // A-B-A 下二者同属 peak-only 的 rosemary 链 → 同在第一/三班（reuse），
        // 恢复班（第二班）排除高峰轮换岗 → 二者同时缺席。
        let (blueprint, operbox, instances, table) = fixtures_243_2gold();
        if !operbox.owns("迷迭香") || !operbox.owns("黑键") {
            return;
        }
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

        let in_shift = |i: usize, name: &str| {
            assignment_operator_names(&report.shifts[i].assignment).contains(name)
        };

        // 第一/三班（peak）：二者同时在岗。
        for i in [0usize, 2usize] {
            assert!(in_shift(i, "迷迭香"), "shift{} 应有迷迭香", i + 1);
            assert!(in_shift(i, "黑键"), "shift{} 应有黑键", i + 1);
        }
        // 第二班（recovery）：二者同时休息。
        assert!(!in_shift(1, "迷迭香"), "恢复班不应有迷迭香");
        assert!(!in_shift(1, "黑键"), "恢复班不应有黑键");
        // 同上同下：任一班次中二者要么同在、要么同不在。
        for shift in &report.shifts {
            let names = assignment_operator_names(&shift.assignment);
            assert_eq!(
                names.contains("迷迭香"),
                names.contains("黑键"),
                "shift{} 迷迭香与黑键应同上同下",
                shift.index + 1
            );
        }
    }

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
}
