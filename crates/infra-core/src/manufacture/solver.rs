use serde::Serialize;

use crate::error::Result;
use crate::manufacture::input::{ManuLineScenario, ManuRoomInput};
use crate::manufacture::interpreter::{apply_manu_phases, ManuContext};
use crate::skill_table::SkillTable;
use crate::types::RecipeKind;

#[derive(Debug, Clone, Serialize, Default)]
pub struct ManuProdBreakdown {
    pub gold: f64,
    pub battle_record: f64,
    pub originium: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuCompositeScore {
    pub composite: f64,
    pub per_station: ManuProdBreakdown,
    pub storage: ManuStorageBreakdown,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ManuStorageBreakdown {
    pub gold: i32,
    pub battle_record: i32,
    pub originium: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperatorMoodDrain {
    pub name: String,
    pub drain_delta_per_hour: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuResult {
    pub active_recipe: RecipeKind,
    pub prod_base: f64,
    pub prod_skill: f64,
    pub prod_total: f64,
    pub storage_limit: i32,
    pub mood_drain: Vec<OperatorMoodDrain>,
}

pub fn solve_manufacture(input: &ManuRoomInput, table: &SkillTable) -> Result<ManuResult> {
    let mut ctx = ManuContext::from_room(input);
    apply_manu_phases(&mut ctx, table);

    let recipe = input.active_recipe;
    Ok(ManuResult {
        active_recipe: recipe,
        prod_base: ctx.prod_base(),
        prod_skill: ctx.prod_skill(recipe),
        prod_total: ctx.prod_total(recipe),
        storage_limit: ctx.storage_limit(recipe),
        mood_drain: ctx
            .mood_drain_summary()
            .into_iter()
            .map(|(name, drain_delta_per_hour)| OperatorMoodDrain {
                name,
                drain_delta_per_hour,
            })
            .collect(),
    })
}

/// 按 `ManuLineScenario` 对同一三人组在各配方产线上求值并加权求和。
pub fn score_manu_composite(
    input: &ManuRoomInput,
    table: &SkillTable,
    scenario: ManuLineScenario,
) -> Result<ManuCompositeScore> {
    let mut composite = 0.0;
    let mut per_station = ManuProdBreakdown::default();
    let mut storage = ManuStorageBreakdown::default();

    for (recipe, lines) in scenario.active_recipes() {
        let mut room = input.clone();
        room.active_recipe = recipe;
        let result = solve_manufacture(&room, table)?;
        let weight = f64::from(lines);
        composite += weight * result.prod_total;
        match recipe {
            RecipeKind::Gold => {
                per_station.gold = result.prod_total;
                storage.gold = result.storage_limit;
            }
            RecipeKind::BattleRecord => {
                per_station.battle_record = result.prod_total;
                storage.battle_record = result.storage_limit;
            }
            RecipeKind::Originium => {
                per_station.originium = result.prod_total;
                storage.originium = result.storage_limit;
            }
            RecipeKind::All => {}
        }
    }

    Ok(ManuCompositeScore {
        composite,
        per_station,
        storage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::manufacture::input::ManuOperator;
    use crate::tier::PromotionTier;

    fn table() -> SkillTable {
        SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap()
    }

    #[test]
    fn three_standard_ops_battle_record() {
        let instances =
            OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = table();
        let names = ["蛇屠箱", "黑角", "米格鲁"];
        let operators: Vec<ManuOperator> = names
            .iter()
            .map(|name| {
                let buff_ids =
                    instances.resolve_manufacture_buff_ids(name, PromotionTier::Tier0);
                ManuOperator::new(*name, 0, buff_ids)
            })
            .collect();
        let input = ManuRoomInput::with_operators(3, RecipeKind::BattleRecord, operators);
        let result = solve_manufacture(&input, &table).unwrap();
        assert!((result.prod_base - 3.0).abs() < 0.01);
        assert!(result.prod_skill > 20.0);
        assert!(result.storage_limit >= 26);
    }
}
