//! 回收站：静态效率求值。
//!
//! 只取知识库《回收站机制》的设施等级、容量、工位、效率与心情；积分、特殊池、投料、
//! 满仓暂停、涣散期计时与产物估值都不进求解。
//!
//! E = 等级基础 + 0.01 × 进驻人数 + Σ 回收站技能加成。回收站技能由
//! `OperatorInstances::resolve_recycle_buff_ids` 取 buff，不绕过 tier 解析：精二旅骨的
//! 「捡海人」替换「拾荒者」，只贡献 +0.30。

use crate::efficiency::Efficiency;
use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{recycle_base_efficiency_millis, recycle_operator_capacity, AssignedOperator};
use crate::skill_table::SkillTable;
use crate::types::Action;

/// 每名进驻干员提供的人数加成（+1 个百分点）。
const PER_OPERATOR_MILLIS: i32 = 10;

/// 一座回收站在给定编制下的效率拆分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecycleEfficiency {
    pub operator_count: usize,
    /// 等级基础 + 0.01 × 进驻人数。
    pub base_efficiency: Efficiency,
    /// 回收站技能加成之和。
    pub operator_efficiency: Efficiency,
    /// `base_efficiency + operator_efficiency`。
    pub total_efficiency: Efficiency,
}

/// 单个干员的回收站技能加成（只读技能表，不含人数加成）。
pub fn recycle_skill_efficiency(
    instances: &OperatorInstances,
    table: &SkillTable,
    operator: &AssignedOperator,
) -> Result<Efficiency> {
    let mut total = Efficiency::ZERO;
    for buff_id in instances.resolve_recycle_buff_ids(&operator.name, operator.tier()) {
        let skill = table.get(&buff_id).ok_or_else(|| {
            Error::msg(format!(
                "{} references unknown skill_table id {buff_id}",
                operator.name
            ))
        })?;
        for atom in &skill.atoms {
            if let Action::AddFlatEff { value, .. } = &atom.action {
                total += Efficiency::from_percent_points(*value);
            }
        }
    }
    Ok(total)
}

/// 回收站效率。`operators` 超过等级容量时返回错误：容量是 hard constraint。
pub fn evaluate_recycle(
    level: u8,
    operators: &[AssignedOperator],
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<RecycleEfficiency> {
    let capacity = recycle_operator_capacity(level);
    if operators.len() > capacity {
        return Err(Error::msg(format!(
            "recycle station level {level} holds at most {capacity} operators, got {}",
            operators.len()
        )));
    }
    let mut operator_efficiency = Efficiency::ZERO;
    for operator in operators {
        operator_efficiency += recycle_skill_efficiency(instances, table, operator)?;
    }
    let base_efficiency = Efficiency::from_millis(
        recycle_base_efficiency_millis(level) + PER_OPERATOR_MILLIS * operators.len() as i32,
    );
    Ok(RecycleEfficiency {
        operator_count: operators.len(),
        base_efficiency,
        operator_efficiency,
        total_efficiency: base_efficiency + operator_efficiency,
    })
}
