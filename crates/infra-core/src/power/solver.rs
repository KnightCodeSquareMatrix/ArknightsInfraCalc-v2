use serde::Serialize;

use crate::eff_ramp::{eff_ramp_at_shift_hours, EffRampStyle};
use crate::efficiency::Efficiency;
use crate::error::Result;
use crate::global_resource::GlobalResourceKey;
use crate::instances::OperatorInstances;
use crate::layout::LayoutContext;
use crate::layout::{RoomId, WorkforceIndex};
use crate::power::input::{PowerOperator, PowerRoomInput};
use crate::power::interpreter::{apply_power_phases, PowerContext};
use crate::skill_table::SkillTable;

const ADDITION_ALPHA: &str = "power_rec_spd&addition[000]";
const ADDITION_BETA: &str = "power_rec_spd&addition[001]";

#[derive(Debug, Clone, Serialize)]
pub struct PowerResult {
    /// 发电岗位自身增量；全基建共享的 `1.000` 由跨站聚合注入。
    pub base_efficiency: Efficiency,
    pub skill_efficiency: Efficiency,
    pub ramp_efficiency: Efficiency,
    /// 当前发电岗位的非共享效率增量。
    pub final_efficiency: Efficiency,
    pub mood_drain_delta: f64,
    pub virtual_power_produced: f64,
}

/// 汇总物理发电站的岗位增量，并只注入一次全基建共享基础效率。
pub fn aggregate_power_efficiency(increments: impl IntoIterator<Item = Efficiency>) -> Efficiency {
    let total = increments.into_iter().sum::<Efficiency>();
    if total.is_zero() {
        Efficiency::ZERO
    } else {
        Efficiency::ONE + total
    }
}

/// 逐发电站求值，将各站 `StateWrite` 产出的虚拟发电站等资源写回 `layout.global`。
pub fn apply_power_to_layout(
    layout: &mut LayoutContext,
    rooms: &[(RoomId, PowerOperator)],
    table: &SkillTable,
    mood: f64,
    shift_hours: f64,
    workforce: &WorkforceIndex,
    instances: &OperatorInstances,
) -> Result<Vec<PowerResult>> {
    let mut results = Vec::with_capacity(rooms.len());
    for (room_id, operator) in rooms {
        let room_layout =
            workforce.layout_for_power_room(layout, room_id, &operator.name, Some(instances));
        let input = PowerRoomInput {
            operator: operator.clone(),
            mood: operator.work_mood.unwrap_or(mood),
            shift_hours,
            layout: room_layout,
        };
        let result = solve_power(&input, table)?;
        if result.virtual_power_produced > 0.0 {
            layout.global.add(
                GlobalResourceKey::VirtualPower,
                result.virtual_power_produced,
            );
        }
        results.push(result);
    }
    Ok(results)
}

pub fn solve_power(input: &PowerRoomInput, table: &SkillTable) -> Result<PowerResult> {
    let mut ctx = PowerContext::from_room(input);
    let before_vp = ctx
        .state_pool
        .get(&GlobalResourceKey::VirtualPower)
        .copied()
        .unwrap_or(0.0);
    apply_power_phases(&mut ctx, table);

    let ramp = charge_ramp_from_buffs(&ctx.operator.buff_ids, ctx.shift_hours);
    let base = ctx.operator.charge_speed_pct;
    let after_vp = ctx
        .state_pool
        .get(&GlobalResourceKey::VirtualPower)
        .copied()
        .unwrap_or(0.0);

    let zero_mood_work = input.operator.work_mood.unwrap_or(input.mood) <= 0.0;
    let skill_efficiency = if zero_mood_work {
        Efficiency::ZERO
    } else {
        Efficiency::from_percent_points(base)
    };
    let ramp_efficiency = if zero_mood_work {
        Efficiency::ZERO
    } else {
        Efficiency::from_percent_points(ramp)
    };
    Ok(PowerResult {
        base_efficiency: Efficiency::ZERO,
        skill_efficiency,
        ramp_efficiency,
        final_efficiency: skill_efficiency + ramp_efficiency,
        mood_drain_delta: ctx.operator.mood_drain_delta,
        virtual_power_produced: (after_vp - before_vp).max(0.0),
    })
}

/// 空构·技术交流：首小时 `initial`，此后每小时 +`per_hour`，上限 `cap`。
pub fn charge_ramp_from_buffs(buff_ids: &[String], shift_hours: f64) -> f64 {
    if buff_ids.iter().any(|b| b == ADDITION_BETA) {
        eff_ramp_at_shift_hours(
            EffRampStyle::FirstHourThenHourly,
            15.0,
            1.0,
            20.0,
            shift_hours,
        )
    } else if buff_ids.iter().any(|b| b == ADDITION_ALPHA) {
        eff_ramp_at_shift_hours(
            EffRampStyle::FirstHourThenHourly,
            10.0,
            1.0,
            15.0,
            shift_hours,
        )
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::power::input::PowerOperator;
    use crate::tier::PromotionTier;

    fn table() -> SkillTable {
        SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap()
    }

    #[test]
    fn castle3_flat_ten_pct() {
        let table = table();
        let input = PowerRoomInput::with_operator(PowerOperator::new(
            "Castle-3",
            0,
            vec!["power_rec_spd[000]".into()],
        ));
        let result = solve_power(&input, &table).unwrap();
        assert_eq!(result.final_efficiency, Efficiency::from_decimal(1.100));
    }

    #[test]
    fn kong_ramp_at_24h() {
        let ramp = charge_ramp_from_buffs(&[ADDITION_BETA.into()], 24.0);
        assert!((ramp - 20.0).abs() < 0.01);
    }

    #[test]
    fn instances_power_buffs_resolve() {
        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let buff_ids = instances.resolve_power_buff_ids("格雷伊", PromotionTier::Tier0);
        let input = PowerRoomInput::with_operator(PowerOperator::new("格雷伊", 0, buff_ids));
        let result = solve_power(&input, &table).unwrap();
        assert_eq!(result.final_efficiency, Efficiency::from_decimal(1.200));
    }

    #[test]
    fn zero_mood_work_has_no_power_efficiency() {
        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let mut operator = PowerOperator::new(
            "Lancet-2",
            0,
            instances.resolve_power_buff_ids("Lancet-2", PromotionTier::Tier0),
        );
        operator.work_mood = Some(0.0);
        let result = solve_power(
            &PowerRoomInput {
                operator,
                mood: 0.0,
                shift_hours: 12.0,
                layout: LayoutContext::default(),
            },
            &table,
        )
        .unwrap();
        assert_eq!(result.final_efficiency, Efficiency::ZERO);
        assert_eq!(result.skill_efficiency, Efficiency::ZERO);
    }

    #[test]
    fn muelsys_ecology_director_excludes_self_from_rhine_count() {
        use crate::layout::{LayoutContext, TAG_RHINE};

        let table = table();
        let mut op = PowerOperator::new("缪尔赛思", 2, vec!["power_rec_rhine[000]".into()]);
        op.tags = vec![TAG_RHINE.into()];
        let mut layout = LayoutContext::default();
        layout.rhine_life_in_base = 6;
        let mut input = PowerRoomInput::with_operator(op);
        input.layout = layout;
        let result = solve_power(&input, &table).unwrap();
        // +10% 基础 + 5×3%（总数 6，排除自身后满额 5）
        assert!(
            result.final_efficiency == Efficiency::from_decimal(1.250),
            "got {}",
            result.final_efficiency
        );
    }
}
