use std::collections::HashMap;

use crate::office::input::OfficeRoomInput;
use crate::skill_table::SkillTable;
use crate::types::{Action, Condition, Phase, StateKey};

#[derive(Debug, Clone, PartialEq)]
pub struct OfficeResult {
    /// 人脉联络速度加成 %（闪击·语言学等）。
    pub hire_spd_pct: f64,
}

pub fn solve_office(input: &OfficeRoomInput, table: &SkillTable) -> OfficeResult {
    let state_pool: HashMap<StateKey, f64> = input.layout.global.to_room_state();
    let mut hire_spd_pct = 0.0;

    for op in &input.operators {
        for bid in &op.buff_ids {
            let Some(skill) = table.get(bid) else {
                continue;
            };
            if skill.facility != "office" {
                continue;
            }
            for atom in &skill.atoms {
                if atom.phase != Phase::Constant {
                    continue;
                }
                if !condition_met(atom.condition.as_ref(), input.mood) {
                    continue;
                }
                hire_spd_pct += apply_constant_action(&atom.action, &state_pool);
            }
        }
    }

    OfficeResult { hire_spd_pct }
}

fn condition_met(condition: Option<&Condition>, mood: f64) -> bool {
    match condition {
        None => true,
        Some(Condition::MoodAbove { n }) => mood > f64::from(*n),
        Some(Condition::MoodBelowOrEq { n }) => mood <= f64::from(*n),
        _ => false,
    }
}

fn apply_constant_action(action: &Action, state_pool: &HashMap<StateKey, f64>) -> f64 {
    match action {
        Action::AddFlatEff { value, .. } => *value,
        Action::StateConsumeToEff {
            key,
            div,
            multiplier,
        } => {
            let Some(sk) = StateKey::parse(key) else {
                return 0.0;
            };
            let state = state_pool.get(&sk).copied().unwrap_or(0.0);
            if *div <= 0.0 {
                0.0
            } else {
                (state / div).floor() * multiplier.unwrap_or(1.0)
            }
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global_resource::GlobalResourceKey;
    use crate::layout::LayoutContext;
    use crate::office::input::OfficeOperator;
    use crate::skill_table::default_skill_table_path;

    fn table() -> SkillTable {
        SkillTable::load(&default_skill_table_path().unwrap()).unwrap()
    }

    #[test]
    fn blitz_linguistics_scales_with_intelligence_and_usaut() {
        let mut layout = LayoutContext::default();
        layout
            .global
            .set(GlobalResourceKey::IntelligenceReserve, 2.0);
        layout.global.set(GlobalResourceKey::UsautDrink, 1.0);
        let input = OfficeRoomInput {
            operators: vec![OfficeOperator {
                name: "闪击".into(),
                elite: 2,
                buff_ids: vec!["office_rec_spd[000]".into()],
            }],
            mood: 24.0,
            layout,
        };
        let result = solve_office(&input, &table());
        assert!(
            (result.hire_spd_pct - 35.0).abs() < f64::EPSILON,
            "20 + 2×5 + 1×5 = 35, got {}",
            result.hire_spd_pct
        );
    }
}
