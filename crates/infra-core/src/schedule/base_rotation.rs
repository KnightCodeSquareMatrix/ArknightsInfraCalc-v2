use std::sync::Arc;

use serde::Serialize;

use crate::efficiency::Efficiency;
use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::{resolve_base, BaseAssignment, BaseBlueprint};
use crate::manufacture::input::ManuRoomInput;
use crate::manufacture::solve_manufacture_with_shift;
use crate::power::{solve_power, PowerRoomInput};
use crate::skill_table::SkillTable;
use crate::trade::input::TradeRoomInput;
use crate::trade::solve_trade_with_shift;

/// 单房间直接效率快照（用于 CLI 逐房展示）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct RoomEfficiencyLine {
    pub room_id: String,
    pub operator_count: usize,
    /// White: efficiency supplied directly by operators or their skills.
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub operator_efficiency: Efficiency,
    /// Green: occupancy and facility/global environment efficiency.
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub station_efficiency: Efficiency,
    /// Paper total before an order-specific multiplier is applied.
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub total_efficiency: Efficiency,
    /// Actual efficiency after an order-specific multiplier.
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub final_efficiency: Efficiency,
    /// Trade order multiplier; 1.000 for non-trade facilities.
    #[serde(default)]
    pub order_multiplier: Efficiency,
    /// Blue: order-equivalent efficiency after removing base and station terms.
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub equivalent_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_skill_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub manufacture_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub manufacture_skill_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub power_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub power_skill_efficiency: Efficiency,
}

fn is_zero_efficiency(v: &Efficiency) -> bool {
    v.is_zero()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ShiftEfficiencies {
    pub trade_efficiency: Efficiency,
    pub manufacture_efficiency: Efficiency,
    pub power_efficiency: Efficiency,
    /// 各房间效率明细（按 SHIFT_STATION_ORDER 顺序）。
    pub room_lines: Vec<RoomEfficiencyLine>,
}

impl ShiftEfficiencies {
    /// 贸易效率按时长折算（不与制造/发电混合量纲）。
    pub fn weighted_trade(&self, shift_hours: f64) -> Efficiency {
        weighted_for_hours(self.trade_efficiency, shift_hours)
    }
    /// 制造效率按时长折算。
    pub fn weighted_manufacture(&self, shift_hours: f64) -> Efficiency {
        weighted_for_hours(self.manufacture_efficiency, shift_hours)
    }
    /// 发电效率按时长折算。
    pub fn weighted_power(&self, shift_hours: f64) -> Efficiency {
        weighted_for_hours(self.power_efficiency, shift_hours)
    }

    pub fn weighted_trade_for_cycle(&self, shift_hours: f64, cycle_hours: f64) -> Efficiency {
        weighted_for_cycle(self.trade_efficiency, shift_hours, cycle_hours)
    }

    pub fn weighted_manufacture_for_cycle(&self, shift_hours: f64, cycle_hours: f64) -> Efficiency {
        weighted_for_cycle(self.manufacture_efficiency, shift_hours, cycle_hours)
    }

    pub fn weighted_power_for_cycle(&self, shift_hours: f64, cycle_hours: f64) -> Efficiency {
        weighted_for_cycle(self.power_efficiency, shift_hours, cycle_hours)
    }
}

fn weighted_for_hours(efficiency: Efficiency, hours: f64) -> Efficiency {
    weighted_for_cycle(efficiency, hours, 24.0)
}

fn weighted_for_cycle(efficiency: Efficiency, hours: f64, cycle_hours: f64) -> Efficiency {
    let minutes = (hours * 60.0).round() as i64;
    let cycle_minutes = (cycle_hours * 60.0).round() as i64;
    efficiency.scale_ratio(minutes, cycle_minutes)
}

/// 对编制逐房求贸易/制造/发电直接效率（满心情）；`shift_hours` 影响发电爬升与产出折算。
pub fn evaluate_base_assignment_efficiencies(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    shift_hours: f64,
    durin_plan: Option<u8>,
) -> Result<ShiftEfficiencies> {
    let resolved = resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        24.0,
        durin_plan,
    )?;

    let mut trade_efficiency = Efficiency::ZERO;
    let mut manufacture_efficiency = Efficiency::ZERO;
    let mut power_efficiency = Efficiency::ZERO;
    let mut room_lines: Vec<RoomEfficiencyLine> = Vec::new();

    for room in &resolved.trade_rooms {
        let mut line = RoomEfficiencyLine {
            room_id: room.id.0.clone(),
            operator_count: room.operators.len(),
            order_multiplier: Efficiency::ONE,
            ..RoomEfficiencyLine::default()
        };
        if let Some(snapshot) = assignment.efficiency_in(&room.id).filter(|s| s.is_trade()) {
            trade_efficiency += snapshot.trade_final_efficiency;
            line.trade_efficiency = snapshot.trade_final_efficiency;
            line.trade_skill_efficiency = snapshot.trade_skill_efficiency;
            line.operator_efficiency = snapshot.trade_skill_efficiency;
            line.order_multiplier = if snapshot.trade_unit_output_multiplier.is_zero() {
                Efficiency::ONE
            } else {
                snapshot.trade_unit_output_multiplier
            };
            line.total_efficiency = if snapshot.trade_paper_efficiency.is_zero()
                && !snapshot.trade_unit_output_multiplier.is_zero()
            {
                snapshot.trade_final_efficiency.scale_ratio(
                    i64::from(Efficiency::SCALE),
                    i64::from(snapshot.trade_unit_output_multiplier.millis()),
                )
            } else if snapshot.trade_paper_efficiency.is_zero() {
                snapshot.trade_final_efficiency
            } else {
                snapshot.trade_paper_efficiency
            };
            line.final_efficiency = snapshot.trade_final_efficiency;
            line.station_efficiency = if snapshot.trade_occupancy_efficiency.is_zero()
                && snapshot.trade_control_efficiency.is_zero()
            {
                line.total_efficiency - Efficiency::ONE - line.operator_efficiency
            } else {
                snapshot.trade_occupancy_efficiency + snapshot.trade_control_efficiency
            };
            line.equivalent_efficiency =
                snapshot.trade_final_efficiency - Efficiency::ONE - line.station_efficiency;
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
            trade_efficiency += result.efficiency.final_efficiency;
            line.trade_efficiency = result.efficiency.final_efficiency;
            line.trade_skill_efficiency = result.efficiency.paper.skill_efficiency;
            line.operator_efficiency = result.efficiency.paper.skill_efficiency;
            line.total_efficiency = result.efficiency.paper.paper_efficiency;
            line.final_efficiency = result.efficiency.final_efficiency;
            line.station_efficiency = result.efficiency.paper.occupancy_efficiency
                + result.efficiency.paper.control_efficiency;
            line.order_multiplier = result.efficiency.production_basis.unit_output_multiplier;
            line.equivalent_efficiency =
                result.efficiency.final_efficiency - Efficiency::ONE - line.station_efficiency;
        }
        room_lines.push(line);
    }

    for room in &resolved.manu_rooms {
        let mut line = RoomEfficiencyLine {
            room_id: room.id.0.clone(),
            operator_count: room.operators.len(),
            order_multiplier: Efficiency::ONE,
            ..RoomEfficiencyLine::default()
        };
        if let Some(snapshot) = assignment
            .efficiency_in(&room.id)
            .filter(|s| s.is_manufacture())
        {
            manufacture_efficiency += snapshot.manufacture_final_efficiency;
            line.manufacture_efficiency = snapshot.manufacture_final_efficiency;
            line.manufacture_skill_efficiency = snapshot.manufacture_skill_efficiency;
            line.operator_efficiency = snapshot.manufacture_skill_efficiency;
            line.station_efficiency = if snapshot.manufacture_occupancy_efficiency.is_zero()
                && snapshot.manufacture_global_efficiency.is_zero()
            {
                snapshot.manufacture_final_efficiency - Efficiency::ONE - line.operator_efficiency
            } else {
                snapshot.manufacture_occupancy_efficiency + snapshot.manufacture_global_efficiency
            };
            line.total_efficiency =
                Efficiency::ONE + line.operator_efficiency + line.station_efficiency;
            line.final_efficiency = snapshot.manufacture_final_efficiency;
            line.equivalent_efficiency =
                line.manufacture_efficiency - Efficiency::ONE - line.station_efficiency;
        } else if !room.operators.is_empty() {
            let input = ManuRoomInput {
                level: room.level,
                operators: room.operators.clone(),
                active_recipe: room.recipe,
                mood: 24.0,
                layout: Arc::new(room.layout.clone()),
            };
            let result = solve_manufacture_with_shift(&input, table, shift_hours)?;
            manufacture_efficiency += result.final_efficiency;
            line.manufacture_efficiency = result.final_efficiency;
            line.manufacture_skill_efficiency = result.skill_efficiency;
            line.operator_efficiency = result.skill_efficiency;
            line.station_efficiency = result.occupancy_efficiency + result.global_efficiency;
            line.total_efficiency = result.final_efficiency;
            line.final_efficiency = result.final_efficiency;
            line.equivalent_efficiency =
                result.final_efficiency - Efficiency::ONE - line.station_efficiency;
        }
        room_lines.push(line);
    }

    for room in &resolved.power_rooms {
        let mut line = RoomEfficiencyLine {
            room_id: room.id.0.clone(),
            operator_count: 1,
            order_multiplier: Efficiency::ONE,
            ..RoomEfficiencyLine::default()
        };
        let input = PowerRoomInput {
            operator: room.operator.clone(),
            mood: 24.0,
            shift_hours,
            layout: room.layout.clone(),
        };
        let result = solve_power(&input, table)?;
        let efficiency = assignment
            .efficiency_in(&room.id)
            .filter(|s| s.is_power())
            .map_or(result.final_efficiency, |snapshot| {
                snapshot.power_final_efficiency
            });
        power_efficiency += efficiency;
        line.power_efficiency = efficiency;
        line.power_skill_efficiency = result.skill_efficiency;
        line.operator_efficiency = result.skill_efficiency + result.ramp_efficiency;
        line.total_efficiency = efficiency;
        line.final_efficiency = efficiency;
        line.equivalent_efficiency = line.operator_efficiency;
        room_lines.push(line);
    }

    Ok(ShiftEfficiencies {
        trade_efficiency,
        manufacture_efficiency,
        power_efficiency,
        room_lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::RoomEfficiencySnapshot;
    use crate::skill_table::{data_path, default_skill_table_path, SkillTable};

    fn fixtures_243_2gold() -> (BaseBlueprint, OperatorInstances, SkillTable) {
        let blueprint =
            BaseBlueprint::load(&data_path("layout/243_use_this_.json").unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        (blueprint, instances, table)
    }

    use crate::instances::default_instances_path;

    #[test]
    fn evaluate_base_assignment_uses_room_efficiency_snapshot() {
        let (blueprint, instances, table) = fixtures_243_2gold();
        let mut assignment = BaseAssignment::default();
        assignment.set_room_with_efficiency(
            "manu_1",
            vec![
                crate::layout::AssignedOperator::new("芬", 1),
                crate::layout::AssignedOperator::new("克洛丝", 1),
                crate::layout::AssignedOperator::new("泡普卡", 1),
            ],
            Some(RoomEfficiencySnapshot {
                manufacture_final_efficiency: Efficiency::from_decimal(3.330),
                manufacture_skill_efficiency: Efficiency::from_decimal(0.300),
                ..RoomEfficiencySnapshot::default()
            }),
        );

        let scores = evaluate_base_assignment_efficiencies(
            &blueprint,
            &assignment,
            &instances,
            &table,
            24.0,
            None,
        )
        .unwrap();
        let line = scores
            .room_lines
            .iter()
            .find(|line| line.room_id == "manu_1")
            .expect("manu_1 line");
        assert_eq!(
            scores.manufacture_efficiency,
            Efficiency::from_decimal(3.330)
        );
        assert_eq!(
            line.manufacture_skill_efficiency,
            Efficiency::from_decimal(0.300)
        );
        assert_eq!(line.operator_efficiency, Efficiency::from_decimal(0.300));
        assert_eq!(line.station_efficiency, Efficiency::from_decimal(2.030));
        assert_eq!(line.total_efficiency, Efficiency::from_decimal(3.330));
        assert_eq!(line.final_efficiency, Efficiency::from_decimal(3.330));
        assert_eq!(line.order_multiplier, Efficiency::ONE);
        assert_eq!(line.equivalent_efficiency, Efficiency::from_decimal(0.300));
        assert_eq!(line.operator_count, 3);
    }
}
