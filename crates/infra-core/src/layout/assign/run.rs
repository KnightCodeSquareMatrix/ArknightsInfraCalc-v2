//! 单班填房内部运行上下文（ADR 0001 决策 A / `AssignmentRun` 不变式）。
//!
//! 收敛 `blueprint / instances / table / options / durin / assignment / used`，
//! 提供 `resolve_snapshot` 等 helper，避免流水线参数列表继续膨胀。
//! 本结构只用于 `layout::assign` 内部，不对外暴露，也不承载机制公式。

use std::collections::HashSet;
use std::time::Instant;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::BaseBlueprint;
use crate::layout::resolve::resolve_base;
use crate::layout::context::LayoutContext;
use crate::operbox::OperBox;
use crate::skill_table::SkillTable;

use super::AssignBaseOptions;

/// 单班填房运行上下文。`assignment` / `used` 是流水线推进中的可变状态。
pub(super) struct AssignmentRun<'a> {
    pub blueprint: &'a BaseBlueprint,
    pub operbox: &'a OperBox,
    pub instances: &'a OperatorInstances,
    pub table: &'a SkillTable,
    pub options: &'a AssignBaseOptions,
    pub durin_plan: u8,
    pub assignment: BaseAssignment,
    pub used: HashSet<String>,
}

impl<'a> AssignmentRun<'a> {
    pub(super) fn new(
        blueprint: &'a BaseBlueprint,
        operbox: &'a OperBox,
        instances: &'a OperatorInstances,
        table: &'a SkillTable,
        options: &'a AssignBaseOptions,
        assignment: BaseAssignment,
        used: HashSet<String>,
    ) -> Self {
        let durin_plan = operbox.durin_dorm_planning_count(instances);
        Self {
            blueprint,
            operbox,
            instances,
            table,
            options,
            durin_plan,
            assignment,
            used,
        }
    }

    /// 取当前编制的 layout 快照。
    ///
    /// `with_table = false` 复刻流水线首个 producer 快照（仅 instances，不传 skill table）；
    /// `with_table = true` 复刻后续 consumer 搜索前的快照（含 skill table 解析效率）。
    pub(super) fn resolve_snapshot(&self, with_table: bool) -> Result<LayoutContext> {
        let table = if with_table { Some(self.table) } else { None };
        Ok(resolve_base(
            self.blueprint,
            &self.assignment,
            Some(self.instances),
            table,
            self.options.mood,
            Some(self.durin_plan),
        )?
        .layout_snapshot())
    }
}

/// 流水线阶段计时器：收敛原先散落的 `Instant` + `eprintln`。
///
/// 行为说明：保留逐阶段计时，但 stderr 输出格式由本 helper 统一，
/// 不再逐行手写宏（ADR 0001：计时日志收进 helper）。
pub(super) struct StageTimer {
    start: Instant,
    last: Instant,
    label: &'static str,
    stages: Vec<(&'static str, f64)>,
}

impl StageTimer {
    pub(super) fn new(label: &'static str) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last: now,
            label,
            stages: Vec::new(),
        }
    }

    /// 记录一个阶段耗时（自上次 `mark` 起）。
    pub(super) fn mark(&mut self, stage: &'static str) {
        let now = Instant::now();
        let ms = now.duration_since(self.last).as_secs_f64() * 1000.0;
        self.stages.push((stage, ms));
        self.last = now;
    }

    /// 输出全部阶段耗时与总计到 stderr。
    pub(super) fn report(&self) {
        let total = self.last.duration_since(self.start).as_secs_f64() * 1000.0;
        let body = self
            .stages
            .iter()
            .map(|(s, ms)| format!("{s}={ms:.2}ms"))
            .collect::<Vec<_>>()
            .join("  ");
        eprintln!("[计时] {} {body}  {}总计={total:.2}ms", self.label, self.label);
    }
}
