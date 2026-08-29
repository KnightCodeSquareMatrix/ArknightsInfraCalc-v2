//! Deterministic explanations attached to final room assignments.

use serde::Serialize;

use crate::layout::{BaseAssignment, BaseBlueprint};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Note {
    pub code: String,
    pub text: String,
}

/// Add notes from solver-owned facts and final room state.
///
/// This function is intentionally idempotent. It never infers solver decisions
/// from operator names or efficiency rankings.
pub(crate) fn annotate_assignment(_blueprint: &BaseBlueprint, assignment: &mut BaseAssignment) {
    let fallback_names = assignment
        .note_facts
        .control_skillless_fallback_names
        .clone();
    for room in &mut assignment.rooms {
        room.notes.clear();
    }

    if let Some(control) = assignment
        .rooms
        .iter_mut()
        .find(|room| room.room_id.0 == "control")
    {
        if control.operators.len() == 5
            && !fallback_names.is_empty()
            && fallback_names
                .iter()
                .all(|name| control.operators.iter().any(|op| &op.name == name))
        {
            control.notes.push(Note {
                code: "control_skillless_fallback".to_string(),
                text: "中枢使用了没有中枢技能的普通干员补位，以保持岗位满编。".to_string(),
            });
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct NoteFacts {
    pub control_skillless_fallback_names: Vec<String>,
}
