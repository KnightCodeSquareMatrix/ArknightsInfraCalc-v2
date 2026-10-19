use std::sync::Arc;

use crate::efficiency::Efficiency;
use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::{resolve_base, BaseAssignment, BaseBlueprint, FacilityKind};
use crate::manufacture::input::ManuRoomInput;
use crate::manufacture::solve_manufacture_with_shift;
use crate::power::aggregate_power_efficiency;
use crate::power::{solve_power, PowerRoomInput};
use crate::recycle::evaluate_recycle;
use crate::skill_table::SkillTable;
use crate::trade::input::TradeRoomInput;
use crate::trade::solve_trade_with_shift;

use super::rotation_types::{RoomEfficiencyLine, ShiftEfficiencies};

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
    let mut power_increments = Vec::new();
    let mut room_lines: Vec<RoomEfficiencyLine> = Vec::new();

    for room in &resolved.trade_rooms {
        let mut line = RoomEfficiencyLine {
            room_id: room.id.0.clone(),
            operator_count: room.operators.len(),
            order_multiplier: Efficiency::ONE,
            ..RoomEfficiencyLine::default()
        };
        if let Some(snapshot) = assignment
            .efficiency_in(&room.id)
            .filter(|s| s.has_trade_breakdown())
        {
            trade_efficiency += snapshot.trade_total_efficiency;
            line.trade_efficiency = snapshot.trade_total_efficiency;
            line.trade_skill_efficiency = snapshot.trade_skill_efficiency;
            line.operator_efficiency = snapshot.trade_skill_efficiency;
            line.base_efficiency = snapshot.trade_base_efficiency;
            line.global_efficiency = snapshot.trade_control_efficiency;
            line.order_multiplier = if snapshot.trade_unit_output_multiplier.is_zero() {
                Efficiency::ONE
            } else {
                snapshot.trade_unit_output_multiplier
            };
            line.total_efficiency = snapshot.trade_paper_efficiency;
            line.final_efficiency = snapshot.trade_total_efficiency;
            line.trade_output_per_day = snapshot.trade_output_per_day;
            line.gold_consumption_per_day = snapshot.gold_consumption_per_day;
            line.originium_shards_per_day = snapshot.trade_originium_shards_per_day;
            line.trade_equivalent_efficiency = snapshot.trade_equivalent_trade_efficiency;
            line.gold_equivalent_efficiency = snapshot.trade_equivalent_gold_efficiency;
            line.combined_output_value = snapshot.trade_combined_output_value;
            line.station_efficiency = if snapshot.trade_occupancy_efficiency.is_zero()
                && snapshot.trade_control_efficiency.is_zero()
            {
                line.total_efficiency - Efficiency::ONE - line.operator_efficiency
            } else {
                snapshot.trade_occupancy_efficiency + snapshot.trade_control_efficiency
            };
            line.equivalent_efficiency =
                snapshot.trade_total_efficiency - Efficiency::ONE - line.station_efficiency;
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
            trade_efficiency += result.efficiency.total_efficiency;
            line.trade_efficiency = result.efficiency.total_efficiency;
            line.trade_skill_efficiency = result.efficiency.paper.skill_efficiency;
            line.operator_efficiency = result.efficiency.paper.skill_efficiency;
            line.base_efficiency = result.efficiency.paper.base_efficiency;
            line.global_efficiency = result.efficiency.paper.control_efficiency;
            line.total_efficiency = result.efficiency.paper.paper_efficiency;
            line.final_efficiency = result.efficiency.total_efficiency;
            line.trade_output_per_day = result.production.unit.unit_trade_per_day
                * result.efficiency.total_efficiency.as_f64();
            line.gold_consumption_per_day = result.production.unit.gold_consumption_per_day
                * crate::trade::unit_output::GSL_GOLD_UNIT_SCALE
                * result.efficiency.total_efficiency.as_f64();
            line.combined_output_value = result.efficiency.combined_output_value;
            line.originium_shards_per_day = result.production.unit.unit_originium_per_day
                * result.efficiency.total_efficiency.as_f64();
            line.trade_equivalent_efficiency = result.efficiency.trade_equivalent_efficiency;
            line.gold_equivalent_efficiency = result.efficiency.gold_equivalent_efficiency;
            line.station_efficiency = result.efficiency.paper.occupancy_efficiency
                + result.efficiency.paper.control_efficiency;
            line.order_multiplier = result.efficiency.production_basis.unit_output_multiplier;
            line.equivalent_efficiency =
                result.efficiency.total_efficiency - Efficiency::ONE - line.station_efficiency;
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
            .filter(|s| s.has_manufacture_breakdown())
        {
            manufacture_efficiency += snapshot.manufacture_total_efficiency;
            line.manufacture_efficiency = snapshot.manufacture_total_efficiency;
            line.manufacture_skill_efficiency = snapshot.manufacture_skill_efficiency;
            line.operator_efficiency = snapshot.manufacture_skill_efficiency;
            line.base_efficiency = snapshot.manufacture_base_efficiency;
            line.global_efficiency = snapshot.manufacture_global_efficiency;
            line.manufacture_unit_output_per_day = snapshot.manufacture_unit_output_per_day;
            line.manufacture_output_per_day = snapshot.manufacture_output_per_day;
            line.station_efficiency = if snapshot.manufacture_occupancy_efficiency.is_zero()
                && snapshot.manufacture_global_efficiency.is_zero()
            {
                snapshot.manufacture_total_efficiency - Efficiency::ONE - line.operator_efficiency
            } else {
                snapshot.manufacture_occupancy_efficiency + snapshot.manufacture_global_efficiency
            };
            line.total_efficiency =
                Efficiency::ONE + line.operator_efficiency + line.station_efficiency;
            line.final_efficiency = snapshot.manufacture_total_efficiency;
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
            line.base_efficiency = result.base_efficiency;
            line.global_efficiency = result.global_efficiency;
            line.manufacture_unit_output_per_day = result.unit_output_per_day;
            line.manufacture_output_per_day = result.output_per_day;
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
        let efficiency = result.final_efficiency;
        power_increments.push(efficiency);
        line.power_efficiency = efficiency;
        line.power_skill_efficiency = result.skill_efficiency;
        line.operator_efficiency = result.skill_efficiency + result.ramp_efficiency;
        line.total_efficiency = efficiency;
        line.final_efficiency = efficiency;
        line.equivalent_efficiency = line.operator_efficiency;
        room_lines.push(line);
    }

    // 回收站：空站也出一行（基础效率照常转化）。不并入贸易/制造/发电三个域汇总，
    // 也不进 daily；`order_multiplier` 必须显式设为 1，否则默认 0 会被序列化。
    for room in blueprint.rooms_of(FacilityKind::Recycle) {
        let evaluated = evaluate_recycle(
            room.level,
            assignment.operators_in(&room.id),
            instances,
            table,
        )?;
        room_lines.push(RoomEfficiencyLine {
            room_id: room.id.0.clone(),
            operator_count: evaluated.operator_count,
            operator_efficiency: evaluated.operator_efficiency,
            base_efficiency: evaluated.base_efficiency,
            total_efficiency: evaluated.total_efficiency,
            order_multiplier: Efficiency::ONE,
            ..RoomEfficiencyLine::default()
        });
    }

    let power_efficiency = aggregate_power_efficiency(power_increments);

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
    fn evaluate_base_assignment_recalculates_incomplete_snapshot() {
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
                manufacture_final_efficiency: Efficiency::from_decimal(9.999),
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
