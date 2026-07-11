use std::fs;
use std::path::Path;

use infra_core::schedule::{TeamShiftResult, TeamLabel};
use infra_core::Error;
use serde::Serialize;

pub const TEAM_SHIFT_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct TeamShiftFile<'a> {
    schema_version: u32,
    kind: &'static str,
    index: usize,
    duration_hours: f64,
    active_teams: &'a [TeamLabel],
    resting_team: TeamLabel,
    assignment: &'a infra_core::layout::BaseAssignment,
    scores: &'a infra_core::schedule::ShiftScores,
    weighted_trade: f64,
    weighted_manu: f64,
    weighted_power: f64,
}

fn document(shift: &TeamShiftResult) -> TeamShiftFile<'_> {
    TeamShiftFile {
        schema_version: TEAM_SHIFT_SCHEMA_VERSION,
        kind: "arknights_infra_team_shift",
        index: shift.index,
        duration_hours: shift.duration_hours,
        active_teams: &shift.active_teams,
        resting_team: shift.resting_team,
        assignment: &shift.assignment,
        scores: &shift.scores,
        weighted_trade: shift.weighted_trade,
        weighted_manu: shift.weighted_manu,
        weighted_power: shift.weighted_power,
    }
}

pub fn write_team_shift(path: &Path, shift: &TeamShiftResult) -> Result<(), Error> {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(&document(shift))?))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use infra_core::layout::BaseAssignment;
    use infra_core::schedule::ShiftScores;

    #[test]
    fn team_shift_document_has_stable_version_and_scores() {
        let shift = TeamShiftResult {
            index: 0,
            duration_hours: 12.0,
            active_teams: vec![TeamLabel::Alpha, TeamLabel::Beta],
            resting_team: TeamLabel::Gamma,
            assignment: BaseAssignment::default(),
            scores: ShiftScores::default(),
            weighted_trade: 0.0,
            weighted_manu: 0.0,
            weighted_power: 0.0,
        };
        let value = serde_json::to_value(document(&shift)).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["kind"], "arknights_infra_team_shift");
        assert!(value.get("assignment").is_some());
        assert!(value.get("scores").is_some());
    }
}
