use crate::layout::blueprint::BaseBlueprint;
use crate::layout::shift::AssignShiftMode;
use crate::operbox::OperBox;

/// 体系完整性判定输入（模拟 / `assign_shift` 之前）。
#[derive(Debug, Clone, Copy)]
pub struct EvaluateContext<'a> {
    pub blueprint: &'a BaseBlueprint,
    pub operbox: &'a OperBox,
    pub mode: AssignShiftMode,
}

impl<'a> EvaluateContext<'a> {
    pub fn new(blueprint: &'a BaseBlueprint, operbox: &'a OperBox, mode: AssignShiftMode) -> Self {
        Self {
            blueprint,
            operbox,
            mode,
        }
    }

    pub fn owns_at_least(&self, name: &str, min_elite: u8) -> bool {
        self.operbox
            .elite_of(name)
            .is_some_and(|e| e >= min_elite)
    }

    pub fn has_facility(&self, kind: crate::layout::blueprint::FacilityKind) -> bool {
        self.blueprint.count_facility(kind) > 0
    }
}
