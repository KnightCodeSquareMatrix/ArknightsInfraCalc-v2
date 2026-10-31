use serde::Serialize;

use crate::efficiency::Efficiency;
use crate::error::{Error, Result};
use crate::layout::FacilityKind;
use crate::manufacture::input::{ManuLineScenario, ManuRoomInput};
use crate::manufacture::interpreter::{apply_manu_phases, ManuContext};
use crate::mood::{default_mood_model, operator_net_drain, DrainInputs};
use crate::skill_table::SkillTable;
use crate::types::{Action, Condition, RecipeKind, Selector};

#[derive(Debug, Clone, Serialize, Default)]
pub struct ManuProdBreakdown {
    pub gold: Efficiency,
    pub battle_record: Efficiency,
    pub originium: Efficiency,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuLineEfficiency {
    #[serde(rename = "total_efficiency")]
    pub final_efficiency: Efficiency,
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
    pub base_efficiency: Efficiency,
    pub occupancy_efficiency: Efficiency,
    pub skill_efficiency: Efficiency,
    pub global_efficiency: Efficiency,
    #[serde(rename = "total_efficiency")]
    pub final_efficiency: Efficiency,
    pub unit_output_per_day: f64,
    pub output_per_day: f64,
    pub storage_limit: i32,
    pub mood_drain: Vec<OperatorMoodDrain>,
}

pub fn solve_manufacture(input: &ManuRoomInput, table: &SkillTable) -> Result<ManuResult> {
    solve_manufacture_at_gaps(input, table, &[])
}

pub fn solve_manufacture_with_shift(
    input: &ManuRoomInput,
    table: &SkillTable,
    shift_hours: f64,
) -> Result<ManuResult> {
    if !shift_hours.is_finite() {
        return Err(Error::msg("manufacture shift duration must be finite"));
    }
    if shift_hours <= 0.0 || !room_has_mood_gap_effect(input, table) {
        return solve_manufacture(input, table);
    }

    let model = default_mood_model()?;
    let start_gap = (model.mood_cap - input.mood).clamp(0.0, model.mood_cap);
    let room_ops: Vec<_> = input
        .operators
        .iter()
        .map(|operator| (operator.name.clone(), operator.elite))
        .collect();
    let control_ops = &input.layout.control_operators;
    let full_control = control_ops.len().max(input.layout.control_workforce.len()) >= 5;
    let recipe = match input.active_recipe {
        RecipeKind::Gold => Some("gold"),
        RecipeKind::BattleRecord => Some("battle_record"),
        RecipeKind::Originium => Some("originium"),
        RecipeKind::All => None,
    };
    let drains: Vec<_> = input
        .operators
        .iter()
        .map(|operator| {
            let drain = operator_net_drain(
                model,
                &DrainInputs {
                    name: &operator.name,
                    elite: operator.elite,
                    facility: FacilityKind::Factory,
                    room_ops: &room_ops,
                    full_control,
                    control_ops,
                    recipe,
                },
            );
            (operator.name.as_str(), drain)
        })
        .collect();

    let mut boundaries = vec![0.0, shift_hours];
    for (operator, (_, drain)) in input.operators.iter().zip(&drains) {
        if *drain <= 0.0 {
            continue;
        }
        for threshold in operator_mood_gap_thresholds(operator, table, model.mood_cap) {
            let elapsed = (threshold - start_gap) / drain;
            if elapsed > 0.0 && elapsed < shift_hours {
                boundaries.push(elapsed);
            }
        }
    }
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup_by(|left, right| (*left - *right).abs() < 1e-9);

    let mut base = 0.0;
    let mut occupancy = 0.0;
    let mut skill = 0.0;
    let mut global = 0.0;
    let mut output = 0.0;
    let mut mood_drain = Vec::new();
    for window in boundaries.windows(2) {
        let duration = window[1] - window[0];
        if duration <= 0.0 {
            continue;
        }
        let elapsed = (window[0] + window[1]) / 2.0;
        let gaps: Vec<_> = drains
            .iter()
            .map(|(name, drain)| {
                (
                    *name,
                    (start_gap + drain * elapsed).clamp(0.0, model.mood_cap),
                )
            })
            .collect();
        let result = solve_manufacture_at_gaps(input, table, &gaps)?;
        base += result.base_efficiency.as_f64() * duration;
        occupancy += result.occupancy_efficiency.as_f64() * duration;
        skill += result.skill_efficiency.as_f64() * duration;
        global += result.global_efficiency.as_f64() * duration;
        output += result.output_per_day * duration;
        mood_drain = result.mood_drain;
    }

    let end_gaps: Vec<_> = drains
        .iter()
        .map(|(name, drain)| {
            (
                *name,
                (start_gap + drain * shift_hours).clamp(0.0, model.mood_cap),
            )
        })
        .collect();
    let end = solve_manufacture_at_gaps(input, table, &end_gaps)?;
    let base_efficiency = Efficiency::from_decimal(base / shift_hours);
    let occupancy_efficiency = Efficiency::from_decimal(occupancy / shift_hours);
    let skill_efficiency = Efficiency::from_decimal(skill / shift_hours);
    let global_efficiency = Efficiency::from_decimal(global / shift_hours);
    Ok(ManuResult {
        active_recipe: input.active_recipe,
        base_efficiency,
        occupancy_efficiency,
        skill_efficiency,
        global_efficiency,
        final_efficiency: base_efficiency + skill_efficiency + global_efficiency,
        unit_output_per_day: unit_output_per_day(input.active_recipe),
        output_per_day: output / shift_hours,
        storage_limit: end.storage_limit,
        mood_drain,
    })
}

fn solve_manufacture_at_gaps(
    input: &ManuRoomInput,
    table: &SkillTable,
    mood_gaps: &[(&str, f64)],
) -> Result<ManuResult> {
    let mut ctx = ManuContext::from_room(input);
    for operator in &mut ctx.operators {
        if let Some((_, gap)) = mood_gaps.iter().find(|(name, _)| *name == operator.name) {
            operator.mood_gap = *gap;
        }
    }
    apply_manu_phases(&mut ctx, table);

    let recipe = input.active_recipe;
    let occupancy_efficiency = Efficiency::from_percent_points(ctx.prod_base());
    let skill_efficiency = Efficiency::from_percent_points(ctx.prod_skill(recipe));
    let global_efficiency = Efficiency::from_percent_points(
        ctx.prod_total(recipe) - ctx.prod_base() - ctx.prod_skill(recipe),
    );
    let unit_output_per_day = unit_output_per_day(recipe);
    let final_efficiency =
        Efficiency::ONE + occupancy_efficiency + skill_efficiency + global_efficiency;
    Ok(ManuResult {
        active_recipe: recipe,
        base_efficiency: Efficiency::ONE + occupancy_efficiency,
        occupancy_efficiency,
        skill_efficiency,
        global_efficiency,
        final_efficiency,
        unit_output_per_day,
        output_per_day: unit_output_per_day * final_efficiency.as_f64(),
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

pub fn unit_output_per_day(recipe: RecipeKind) -> f64 {
    match recipe {
        RecipeKind::Gold => 10_000.0,
        RecipeKind::BattleRecord => 8_000.0,
        RecipeKind::Originium => 24.0,
        RecipeKind::All => 0.0,
    }
}

fn room_has_mood_gap_effect(input: &ManuRoomInput, table: &SkillTable) -> bool {
    input.operators.iter().any(|operator| {
        operator.buff_ids.iter().any(|buff_id| {
            table.get(buff_id).is_some_and(|skill| {
                skill.atoms.iter().any(|atom| {
                    atom.selector == Some(Selector::OwnerMoodGap)
                        || matches!(&atom.condition, Some(Condition::OwnerMoodGapAbove { .. }))
                })
            })
        })
    })
}

fn operator_mood_gap_thresholds(
    operator: &crate::manufacture::input::ManuOperator,
    table: &SkillTable,
    mood_cap: f64,
) -> Vec<f64> {
    let mut thresholds = Vec::new();
    for buff_id in &operator.buff_ids {
        let Some(skill) = table.get(buff_id) else {
            continue;
        };
        for atom in &skill.atoms {
            if let Some(Condition::OwnerMoodGapAbove { n }) = &atom.condition {
                thresholds.push(f64::from(*n));
            }
            if atom.selector == Some(Selector::OwnerMoodGap) {
                if let Action::AddBucketEffFromSelector { step, .. } = &atom.action {
                    if *step > 0.0 {
                        let mut threshold = *step;
                        while threshold <= mood_cap {
                            thresholds.push(threshold);
                            threshold += *step;
                        }
                    }
                }
            }
        }
    }
    thresholds
}

/// 按 `ManuLineScenario` 对同一三人组在各配方产线上求值并加权求和。
pub fn evaluate_manufacture_lines(
    input: &ManuRoomInput,
    table: &SkillTable,
    scenario: ManuLineScenario,
) -> Result<ManuLineEfficiency> {
    let mut final_efficiency = Efficiency::ZERO;
    let mut per_station = ManuProdBreakdown::default();
    let mut storage = ManuStorageBreakdown::default();

    for (recipe, lines) in scenario.active_recipes() {
        let mut room = input.clone();
        room.active_recipe = recipe;
        let result = solve_manufacture(&room, table)?;
        final_efficiency += result.final_efficiency.scale_ratio(i64::from(lines), 1);
        match recipe {
            RecipeKind::Gold => {
                per_station.gold = result.final_efficiency;
                storage.gold = result.storage_limit;
            }
            RecipeKind::BattleRecord => {
                per_station.battle_record = result.final_efficiency;
                storage.battle_record = result.storage_limit;
            }
            RecipeKind::Originium => {
                per_station.originium = result.final_efficiency;
                storage.originium = result.storage_limit;
            }
            RecipeKind::All => {}
        }
    }

    Ok(ManuLineEfficiency {
        final_efficiency,
        per_station,
        storage,
    })
}
