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
    solve_manufacture_inner(input, table, None)
}

pub fn solve_manufacture_with_shift(
    input: &ManuRoomInput,
    table: &SkillTable,
    shift_hours: f64,
) -> Result<ManuResult> {
    solve_manufacture_inner(input, table, Some(shift_hours))
}

fn solve_manufacture_inner(
    input: &ManuRoomInput,
    table: &SkillTable,
    shift_hours: Option<f64>,
) -> Result<ManuResult> {
    let mut ctx = ManuContext::from_room(input);
    apply_manu_phases(&mut ctx, table);

    let recipe = input.active_recipe;
    let prod_base = ctx.prod_base();
    let mut prod_skill = ctx.prod_skill(recipe);
    if let Some(hours) = shift_hours {
        prod_skill = manufacture_shift_effective_prod_skill(input, table, prod_skill, hours);
    }
    let prod_global = ctx.layout.global_inject.manu_eff_for(recipe);
    Ok(ManuResult {
        active_recipe: recipe,
        prod_base,
        prod_skill,
        prod_total: prod_base + prod_skill + prod_global,
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

fn manufacture_shift_effective_prod_skill(
    input: &ManuRoomInput,
    table: &SkillTable,
    paper_prod_skill: f64,
    shift_hours: f64,
) -> f64 {
    if shift_hours <= 0.0 {
        return paper_prod_skill;
    }
    let mut delta = 0.0;
    for op in &input.operators {
        for buff_id in &op.buff_ids {
            delta += blurred_vision_shift_delta(buff_id, table, shift_hours);
        }
    }
    paper_prod_skill + delta
}

fn blurred_vision_shift_delta(buff_id: &str, table: &SkillTable, shift_hours: f64) -> f64 {
    const PREFIX: &str = "manu_prod_spd_reduce[";
    const MOOD_DRAIN_PER_HOUR: f64 = 1.0;
    if !buff_id.starts_with(PREFIX) {
        return 0.0;
    }
    let Some(skill) = table.get(buff_id) else {
        return 0.0;
    };
    if skill.facility != "manufacture" {
        return 0.0;
    }
    let paper_eff: f64 = skill
        .atoms
        .iter()
        .filter_map(|atom| match atom.action {
            crate::types::Action::AddFlatEff { value, .. } => Some(value),
            _ => None,
        })
        .sum();
    if paper_eff <= 0.0 {
        return 0.0;
    }
    let worked = shift_hours.min(24.0);
    let mut elapsed = 0.0;
    let mut penalty_area = 0.0;
    while elapsed < worked {
        let mood_gap = elapsed * MOOD_DRAIN_PER_HOUR;
        let tier = (mood_gap / 4.0).floor();
        let next_gap = (tier + 1.0) * 4.0;
        let next_elapsed = (next_gap / MOOD_DRAIN_PER_HOUR).min(worked);
        penalty_area += (next_elapsed - elapsed) * tier * 5.0;
        elapsed = next_elapsed;
    }
    let avg_penalty = penalty_area / worked;
    -avg_penalty.min(paper_eff)
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
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = table();
        let names = ["蛇屠箱", "黑角", "米格鲁"];
        let operators: Vec<ManuOperator> = names
            .iter()
            .map(|name| {
                let buff_ids = instances.resolve_manufacture_buff_ids(name, PromotionTier::Tier0);
                ManuOperator::new(*name, 0, buff_ids)
            })
            .collect();
        let input = ManuRoomInput::with_operators(3, RecipeKind::BattleRecord, operators);
        let result = solve_manufacture(&input, &table).unwrap();
        assert!((result.prod_base - 3.0).abs() < 0.01);
        assert!(result.prod_skill > 20.0);
        assert!(result.storage_limit >= 26);
    }

    #[test]
    fn reduce_speed_buff_is_scored_by_shift_duration() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![ManuOperator::new(
                "测试衰减",
                0,
                vec!["manu_prod_spd_reduce[000]".into()],
            )],
        );

        let paper = solve_manufacture(&input, &table).unwrap();
        let short = solve_manufacture_with_shift(&input, &table, 6.0).unwrap();
        let long = solve_manufacture_with_shift(&input, &table, 18.0).unwrap();

        assert!((paper.prod_skill - 30.0).abs() < 0.01);
        assert!(short.prod_skill < paper.prod_skill);
        assert!(
            long.prod_skill < short.prod_skill,
            "long shift should down-score mood-gap decay: short={} long={}",
            short.prod_skill,
            long.prod_skill
        );
    }
}
