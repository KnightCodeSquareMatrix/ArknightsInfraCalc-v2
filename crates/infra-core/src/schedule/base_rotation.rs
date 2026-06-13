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
use crate::skill_table::SkillTable;
use crate::trade::input::TradeRoomInput;
use crate::trade::solve_trade_with_shift;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BaseShiftRole {
    Peak,
    Recovery,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShiftScores {
    pub trade_score: f64,
    pub manu_prod_sum: f64,
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

/// 对编制逐房求贸易/制造纸面分（满心情）。
pub fn score_base_assignment(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
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
        trade_score += solve_trade_with_shift(&input, table, 24.0)?.effective_eff_multiplier;
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

    Ok(ShiftScores {
        trade_score,
        manu_prod_sum,
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

    let peak_scores = score_base_assignment(blueprint, &peak_assignment, instances, table, Some(durin_plan))?;
    let recovery_scores =
        score_base_assignment(blueprint, &recovery_assignment, instances, table, Some(durin_plan))?;

    let shift1 = BaseShiftPlan {
        index: 0,
        role: BaseShiftRole::Peak,
        assignment: peak_assignment.clone(),
        scores: peak_scores,
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

    let shift3_scores =
        score_base_assignment(blueprint, &peak_assignment, instances, table, Some(durin_plan))?;
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
